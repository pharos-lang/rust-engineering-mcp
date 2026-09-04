"""Trusted, serial M1-16 broker. Not a model runner or an MCP server.

Host constructs Workspace, Driver(host_config), Broker('A'|'B', ...), then calls
run_participant(prompt, broker.tools(cancel), broker.handle, output_dir). Always
close the driver in finally and record broker.receipt() and driver.cleanup.
Only the host supplies configuration and the identical frozen catalog projection.
"""
import copy
import hashlib
import json
import os
from pathlib import Path
import re
import selectors
import signal
import stat
import subprocess
import threading
import time
import tomllib
import uuid

MAX_LINE = 1024 * 1024
MAX_REPLY = MAX_LINE // 2
MAX_TOTAL = 16 * MAX_LINE
FILES = ('Cargo.toml', 'Cargo.lock', 'src/lib.rs')
TOOLS = ('rust.project.open', 'rust.project.inspect', 'rust.toolchain.inspect',
         'rust.check', 'rust.fmt.check', 'rust.clippy', 'rust.test',
         'rust.dependencies.audit', 'rust.diagnostics.explain', 'rust.quality.gate',
         'rust.catalog.status', 'rust.crate.search', 'rust.crate.inspect')
VALIDATIONS = frozenset(('rust.check', 'rust.fmt.check', 'rust.clippy', 'rust.test',
                         'rust.dependencies.audit', 'rust.quality.gate',
                         'rust.project.inspect', 'rust.toolchain.inspect',
                         'rust.diagnostics.explain'))
BINARY = Path(__file__).resolve().parents[1] / 'debug/m1-16-trusted-driver'
HOST_PROFILE = '(version 1) (allow default) (deny network-outbound (remote ip "*:*"))'


def encoded(value):
    return json.dumps(value, ensure_ascii=True, allow_nan=False,
                      separators=(',', ':')).encode()


class BrokerError(RuntimeError):
    """Infrastructure/security failure; never an ordinary failed validation."""


class Denied(Exception):
    pass


class Driver:
    """Single outstanding bounded IPC request; cancellation joins the Rust owner.

    A forced stop is ALWAYS cleanup_failed. It cannot establish server/container
    cleanup, even if the owned driver subsequently disappears.
    """
    def __init__(self, config, *, startup_seconds=45, call_seconds=900,
                 cleanup_seconds=300):
        self.config = copy.deepcopy(config)
        self.call_seconds = call_seconds
        self.cleanup_seconds = cleanup_seconds
        self.cleanup = None
        self.pending = bytearray()
        self.lines = []
        self.total = 0
        self.stderr_total = 0
        self.stderr_prefix = bytearray()
        self.selector = selectors.DefaultSelector()
        self.process = subprocess.Popen(['/usr/bin/sandbox-exec', '-p', HOST_PROFILE, str(BINARY)], stdin=subprocess.PIPE,
                                        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                        env={}, cwd='/', start_new_session=True)
        for stream, kind in ((self.process.stdout, 'out'), (self.process.stderr, 'err')):
            os.set_blocking(stream.fileno(), False)
            self.selector.register(stream, selectors.EVENT_READ, kind)
        os.set_blocking(self.process.stdin.fileno(), False)
        try:
            self._write(config, threading.Event(), time.monotonic() + startup_seconds)
            self.ready = self._response(threading.Event(), time.monotonic() + startup_seconds)
            if self.ready.get('ready') is not True or self.ready.get('ipc_version') != 1:
                raise BrokerError('driver_not_ready')
        except BaseException:
            self.cancel_and_join()
            raise

    def _pump(self, timeout):
        for key, _ in self.selector.select(timeout):
            try:
                chunk = os.read(key.fileobj.fileno(), 65536)
            except BlockingIOError:
                continue  # Readiness is advisory for a nonblocking descriptor.
            if not chunk:
                self.selector.unregister(key.fileobj)
                continue
            if key.data == 'err':
                self.stderr_total += len(chunk)
                self.stderr_prefix.extend(chunk[:max(0, 65536-len(self.stderr_prefix))])
                if self.stderr_total > MAX_TOTAL:
                    raise BrokerError('driver_stderr_budget')
                continue
            self.total += len(chunk)
            if self.total > MAX_TOTAL:
                raise BrokerError('driver_response_total_budget')
            self.pending.extend(chunk)
            while b'\n' in self.pending:
                end = self.pending.index(b'\n') + 1
                if end > MAX_LINE or len(self.lines) >= 2:
                    raise BrokerError('driver_line_budget')
                self.lines.append(bytes(self.pending[:end]))
                del self.pending[:end]
            if len(self.pending) >= MAX_LINE:
                raise BrokerError('driver_line_budget')

    def _write(self, value, cancel, deadline):
        data = encoded(value) + b'\n'
        if len(data) > MAX_LINE:
            raise BrokerError('driver_request_budget')
        view = memoryview(data)
        while view:
            if cancel.is_set() or time.monotonic() >= deadline:
                raise BrokerError('driver_cancelled_or_deadline')
            try:
                count = os.write(self.process.stdin.fileno(), view)
                view = view[count:]
            except BlockingIOError:
                self._pump(.02)

    def _response(self, cancel, deadline):
        while not self.lines:
            if cancel.is_set() or time.monotonic() >= deadline:
                raise BrokerError('driver_cancelled_or_deadline')
            self._pump(.05)
            if self.process.poll() is not None and not self.lines:
                raise BrokerError('driver_eof')
        try:
            answer = json.loads(self.lines.pop(0), parse_constant=lambda _: (_ for _ in ()).throw(ValueError()))
        except (ValueError, UnicodeError) as exc:
            raise BrokerError('driver_invalid_json') from exc
        if not isinstance(answer, dict):
            raise BrokerError('driver_failed')
        if 'driver_error' in answer:
            if 'cleanup_uncertain' in answer:
                self.terminal_ack = answer
            raise BrokerError('driver_failed')
        return answer

    def request(self, payload, cancel):
        if self.cleanup is not None:
            raise BrokerError('driver_closed')
        try:
            deadline = time.monotonic() + self.call_seconds
            self._write(payload, cancel, deadline)
            return self._response(cancel, deadline)
        except BaseException:
            self.cancel_and_join()
            raise

    def _join(self, cancelled, acknowledgement=None):
        acknowledgement = acknowledgement or getattr(self, 'terminal_ack', None)
        deadline = time.monotonic() + self.cleanup_seconds
        forced = False
        while self.process.poll() is None and time.monotonic() < deadline:
            try:
                self._pump(.05)
                # Cancellation output is not a successful request response.
                for line in self.lines:
                    try:
                        observed = json.loads(line)
                        if observed.get('execution_joined') is True:
                            acknowledgement = observed
                    except (ValueError, AttributeError):
                        pass
                self.lines.clear()
            except BrokerError:
                # Still join execution owners after a transport budget failure.
                self.lines.clear()
                self.pending.clear()
        if self.process.poll() is None:
            forced = True
            self.process.kill()
        try:
            code = self.process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            code = None
            forced = True
        drain_deadline = time.monotonic() + 1
        while self.selector.get_map() and time.monotonic() < drain_deadline:
            try:
                self._pump(.01)
            except BrokerError:
                self.pending.clear()
                break
        for line in self.lines:
            try:
                observed = json.loads(line)
                if observed.get('execution_joined') is True:
                    acknowledgement = observed
            except (ValueError, AttributeError):
                pass
        server_joined = (acknowledgement or {}).get('server_joined') is True
        joined = bool(not forced and acknowledgement and
                      (self.config['mode'] == 'raw' or server_joined) and
                      acknowledgement.get('cleanup_uncertain') is False and
                      acknowledgement.get('execution_joined') is True
                      and ((code == 0 and acknowledgement.get('closed') is True and 'driver_error' not in acknowledgement)
                           or (cancelled and code == 1 and acknowledgement.get('driver_error') == 'cancelled')))
        self.cleanup = {'cancelled': cancelled, 'forced_driver_stop': forced,
                        'exit_code': code, 'execution_joined': not forced and (acknowledgement or {}).get('execution_joined') is True,
                        'driver_execution_joined_claim': (acknowledgement or {}).get('execution_joined'),
                        'cleanup_verified': joined,
                        'cleanup_failed': not joined,
                        'server_cleanup_verified': (joined and server_joined) if self.config['mode'] == 'mcp' else None,
                        'server_joined': server_joined,
                        'cleanup_uncertain': (acknowledgement or {}).get('cleanup_uncertain'),
                        'terminal_error': (acknowledgement or {}).get('driver_error'),
                        'observed_service_exit': (acknowledgement or {}).get('server_exit'),
                        'stderr_bytes': self.stderr_total,
                        'stderr_sha256': hashlib.sha256(self.stderr_prefix).hexdigest()}
        for stream in (self.process.stdin, self.process.stdout, self.process.stderr):
            stream.close()
        self.selector.close()
        return copy.deepcopy(self.cleanup)

    def cancel_and_join(self):
        if self.cleanup is not None:
            return copy.deepcopy(self.cleanup)
        if self.process.poll() is None:
            try:
                os.kill(self.process.pid, signal.SIGINT)
            except ProcessLookupError:
                pass
        return self._join(True)

    def close(self):
        if self.cleanup is not None:
            return copy.deepcopy(self.cleanup)
        try:
            ack = self.request({'op': 'close'}, threading.Event())
            if ack.get('closed') is not True:
                raise BrokerError('driver_close_unacknowledged')
            return self._join(False, ack)
        except BaseException:
            self.cancel_and_join()
            raise


def _node(info):
    return info.st_dev, info.st_ino


def _open_dir(path):
    path = Path(path)
    if not path.is_absolute() or '..' in path.parts:
        raise BrokerError('host_parent_not_absolute')
    fd = os.open('/', os.O_RDONLY | os.O_DIRECTORY)
    try:
        for part in path.parts[1:]:
            next_fd = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                              dir_fd=fd)
            os.close(fd)
            fd = next_fd
        info = os.fstat(fd)
        if info.st_uid != os.getuid() or stat.S_IMODE(info.st_mode) != 0o700:
            raise BrokerError('host_parent_not_private')
        return fd
    except BaseException:
        os.close(fd)
        raise


class Workspace:
    """Owns fresh root and separate private candidate artifacts; never follows links."""
    def __init__(self, parent, initial_files):
        if set(initial_files) != set(FILES):
            raise BrokerError('initial_closure_must_be_exact')
        self.parent_path = Path(parent)
        self.parent_fd = _open_dir(parent)
        self.name = 'workspace-' + uuid.uuid4().hex
        self.artifact_name = 'candidates-' + uuid.uuid4().hex
        self.fds = [self.parent_fd]
        self.initial = copy.deepcopy(initial_files)
        for name in (self.name, self.artifact_name):
            os.mkdir(name, mode=0o700, dir_fd=self.parent_fd)
        self.root_fd = self._dir(self.parent_fd, self.name)
        self.artifact_fd = self._dir(self.parent_fd, self.artifact_name)
        os.mkdir('src', mode=0o700, dir_fd=self.root_fd)
        self.src_fd = self._dir(self.root_fd, 'src')
        self.root = self.parent_path / self.name
        self.artifacts = self.parent_path / self.artifact_name
        for name, value in initial_files.items():
            data = self._source(value, 32768 if name == 'src/lib.rs' else 65536)
            fd, leaf = self._file_location(name)
            self._new(fd, leaf, data)
        self.candidates = []

    def _dir(self, parent, name):
        fd = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=parent)
        self.fds.append(fd)
        return fd

    @staticmethod
    def _source(value, cap=32768):
        if not isinstance(value, str):
            raise Denied('source_must_be_utf8_string')
        try:
            data = value.encode('utf-8')
        except UnicodeError as exc:
            raise Denied('source_must_be_utf8_string') from exc
        if len(data) > cap or b'\0' in data:
            raise Denied('source_byte_budget_or_nul')
        return data

    @staticmethod
    def _new(fd, name, data):
        out = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                      0o600, dir_fd=fd)
        try:
            view = memoryview(data)
            while view:
                count = os.write(out, view)
                view = view[count:]
            os.fsync(out)
        finally:
            os.close(out)

    def _check(self):
        try:
            fresh_parent = _open_dir(self.parent_path)
            try:
                if _node(os.fstat(fresh_parent)) != _node(os.fstat(self.parent_fd)):
                    raise BrokerError('workspace_parent_replaced')
            finally:
                os.close(fresh_parent)
            for parent, name, fd in ((self.parent_fd, self.name, self.root_fd),
                                     (self.parent_fd, self.artifact_name, self.artifact_fd),
                                     (self.root_fd, 'src', self.src_fd)):
                current = os.stat(name, dir_fd=parent, follow_symlinks=False)
                if (not stat.S_ISDIR(current.st_mode) or current.st_uid != os.getuid()
                        or stat.S_IMODE(current.st_mode) != 0o700
                        or _node(current) != _node(os.fstat(fd))):
                    raise BrokerError('workspace_directory_replaced')
        except OSError as exc:
            raise BrokerError('workspace_directory_unavailable') from exc

    def _file_location(self, name):
        if name not in FILES:
            raise Denied('file_not_in_visible_closure')
        return (self.src_fd, 'lib.rs') if name == 'src/lib.rs' else (self.root_fd, name)

    def read(self, name):
        self._check()
        parent, leaf = self._file_location(name)
        try:
            fd = os.open(leaf, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=parent)
            try:
                before = os.fstat(fd)
                if (not stat.S_ISREG(before.st_mode) or before.st_nlink != 1 or before.st_size > 65536
                        or before.st_uid != os.getuid() or stat.S_IMODE(before.st_mode) != 0o600):
                    raise BrokerError('unsafe_workspace_file')
                data = bytearray()
                while len(data) <= 65536:
                    chunk = os.read(fd, min(8192, 65537-len(data)))
                    if not chunk:
                        break
                    data.extend(chunk)
                after = os.fstat(fd)
                stamp = lambda s: (_node(s), s.st_size, s.st_mtime_ns, s.st_ctime_ns, s.st_nlink)
                named = os.stat(leaf, dir_fd=parent, follow_symlinks=False)
                if len(data) > 65536 or stamp(before) != stamp(after) or _node(named) != _node(after):
                    raise BrokerError('workspace_file_changed')
            finally:
                os.close(fd)
            self._check()
            text = data.decode('utf-8')
            if name != 'src/lib.rs' and text != self.initial[name]:
                raise BrokerError('immutable_file_changed')
            return text
        except (OSError, UnicodeError) as exc:
            raise BrokerError('workspace_file_unavailable') from exc

    def snapshot(self):
        return [{'path': name, 'text': self.read(name)} for name in FILES]

    def submit(self, source):
        data = self._source(source)
        if len(self.candidates) >= 6:
            raise Denied('candidate_budget_exhausted')
        self.snapshot()
        number = len(self.candidates) + 1
        self._new(self.artifact_fd, f'candidate-{number:02d}.rs', data)
        self._new(self.src_fd, '.broker-pending', data)
        self._check()
        os.replace('.broker-pending', 'lib.rs', src_dir_fd=self.src_fd, dst_dir_fd=self.src_fd)
        os.fsync(self.src_fd)
        self._check()
        entry = {'candidate': number, 'sha256': hashlib.sha256(data).hexdigest(),
                 'bytes': len(data), 'kind': 'patch'}
        self.candidates.append(entry)
        return copy.deepcopy(entry)

    def close(self):
        for fd in reversed(self.fds):
            os.close(fd)
        self.fds.clear()


def _schema(name, description, properties, required):
    return {'name': name, 'description': description, 'inputSchema': {
        'type': 'object', 'additionalProperties': False,
        'properties': properties, 'required': required}}


class Broker:
    def __init__(self, arm, workspace, driver, catalog_facts, *, strict_clippy=True, allow_project_code=False):
        if arm not in ('A', 'B'):
            raise BrokerError('invalid_arm')
        if driver.config['mode'] != ('raw' if arm == 'A' else 'mcp') or Path(driver.config['root']) != workspace.root:
            raise BrokerError('driver_workspace_or_arm_mismatch')
        self.arm, self.workspace, self.driver = arm, workspace, driver
        self.catalog_facts = copy.deepcopy(catalog_facts)
        if len(encoded(self.catalog_facts)) > MAX_REPLY - 1024:
            raise BrokerError('catalog_projection_budget')
        self.strict_clippy = strict_clippy
        self.allow_project_code = allow_project_code is True
        self.submissions = []
        self.calls = 0
        self.validations = []
        self.selection = None
        self.selection_count = 0
        self.names = {}
        self.declarations = None
        self.events = []

    def tools(self, cancel):
        if self.declarations is not None:
            return copy.deepcopy(self.declarations)
        shared = [
            _schema('read_project_file', 'Read one immutable manifest/lock or current editable source.',
                    {'file': {'type': 'string', 'enum': list(FILES)}}, ['file']),
            _schema('submit_patch', 'Submit a complete replacement src/lib.rs; six candidates total, UTF8 <=32768 bytes.',
                    {'source': {'type': 'string', 'maxLength': 32768}}, ['source']),
            _schema('submit_selection', 'Submit a crate/version choice and evidence; six candidates total.',
                    {'name': {'type': 'string', 'maxLength': 128},
                     'version': {'type': 'string', 'maxLength': 128},
                     'evidence': {'type': 'string', 'maxLength': 8192}}, ['name', 'version', 'evidence'])]
        if self.arm == 'A':
            shared.extend([
                _schema('raw_validate', 'Run fixed sandboxed offline validation; quality bundles fmt/check/strict clippy/test plus a std-only lock fact. Six validation requests total.',
                        {'stage': {'type': 'string', 'enum': ['check', 'fmt', 'clippy', 'test', 'metadata', 'quality']}}, ['stage']),
                _schema('read_catalog_facts', 'Read the identical frozen catalog fact projection.', {}, []),
                _schema('raw_explain', 'Read rustc explanation for a bounded compiler diagnostic code.',
                        {'code': {'type': 'string', 'pattern': '^E[0-9]{4}$'}}, ['code'])])
        else:
            discovery = self.driver.request({'op': 'tools'}, cancel)
            found = discovery.get('tools')
            if not isinstance(found, list) or len(found) != 13 or {t.get('name') for t in found} != set(TOOLS) or discovery.get('nextCursor'):
                raise BrokerError('unexpected_mcp_discovery')
            for tool in found:
                name = tool['name'].replace('.', '_')
                schema = tool.get('inputSchema')
                if not isinstance(schema, dict) or schema.get('type') != 'object' or schema.get('additionalProperties') is not False:
                    raise BrokerError('mcp_schema_not_closed_object')
                self.names[name] = tool['name']
                shared.append({'name': name, 'description': tool.get('description', ''),
                               'inputSchema': copy.deepcopy(schema)})
            shared.append(_schema('resource_read', 'Read an owner-authorized Rust log Resource through the same MCP session.',
                                  {'uri': {'type': 'string', 'pattern': '^rust-artifact://prj_[0-9a-f]{32}/art_[0-9a-f]{32}$'}}, ['uri']))
        self.declarations = shared
        return copy.deepcopy(shared)

    def _validation(self, name, operation):
        if not self.allow_project_code:
            raise Denied('host_project_code_consent_required')
        if len(self.validations) >= 6:
            raise Denied('validation_budget_exhausted')
        record = {'request': len(self.validations)+1, 'name': name,
                  'candidate': len(self.submissions), 'stages': []}
        self.validations.append(record)
        start = time.monotonic()
        try:
            result = operation(record)
            record['result'] = copy.deepcopy(result)
            return result
        finally:
            record['elapsed_ms'] = round((time.monotonic()-start)*1000, 3)

    def _raw(self, stage, cancel, record):
        commands = ['fmt', 'check', 'clippy', 'test'] if stage == 'quality' else [stage]
        results = []
        for command in commands:
            if cancel.is_set():
                self.driver.cancel_and_join()
                raise BrokerError('cancelled')
            start = time.monotonic()
            result = self.driver.request({'op': 'execute', 'files': self.workspace.snapshot(), 'command': command}, cancel)
            record['stages'].append({'stage': command, 'elapsed_ms': round((time.monotonic()-start)*1000, 3)})
            results.append({'stage': command, 'result': result})
        if stage != 'quality':
            return results[0]['result']
        lock = tomllib.loads(self.workspace.read('Cargo.lock'))
        manifest = tomllib.loads(self.workspace.read('Cargo.toml'))
        packages = lock.get('package', [])
        package = manifest.get('package', {})
        std_only = (len(packages) == 1 and packages[0].get('name') == package.get('name')
                    and packages[0].get('version') == package.get('version')
                    and not packages[0].get('dependencies') and not packages[0].get('source'))
        return {'stages': results, 'lock_audit_fact': {
            'std_only_locked_closure': std_only, 'third_party_locked_packages': max(0, len(packages)-1) if std_only else None,
            'cargo_audit_executed': False,
            'lock_sha256': hashlib.sha256(self.workspace.read('Cargo.lock').encode()).hexdigest()}}

    def handle(self, name, args, cancel):
        if cancel.is_set():
            self.driver.cancel_and_join()
            raise BrokerError('cancelled')
        self.calls += 1
        if self.calls > 64:
            return {'broker_error': 'request_budget_exhausted', 'retryable': False}
        start = time.monotonic()
        try:
            result = self._handle(name, args, cancel)
            if cancel.is_set():
                self.driver.cancel_and_join()
                # The operation may already have committed a candidate. Preserve
                # its result; the participant records late delivery separately,
                # and driver cleanup remains an orthogonal runner gate.
            if len(encoded(result)) > MAX_REPLY:
                raise BrokerError('participant_response_budget')
            return result
        except Denied as exc:
            return {'broker_error': str(exc), 'retryable': True}
        finally:
            self.events.append({'request': self.calls, 'name': name,
                                'elapsed_ms': round((time.monotonic()-start)*1000, 3)})

    def _handle(self, name, args, cancel):
        declarations = {t['name']: t for t in self.tools(cancel)}
        if name not in declarations or not isinstance(args, dict):
            raise Denied('tool_or_arguments_denied')
        schema = declarations[name]['inputSchema']
        if set(args)-set(schema.get('properties', {})) or set(schema.get('required', []))-set(args):
            raise Denied('unexpected_or_missing_argument')
        if name == 'read_project_file':
            if not isinstance(args['file'], str):
                raise Denied('invalid_file')
            return {'file': args['file'], 'text': self.workspace.read(args['file'])}
        if name == 'submit_patch':
            if len(self.workspace.candidates) + self.selection_count >= 6:
                raise Denied('candidate_budget_exhausted')
            entry = self.workspace.submit(args['source'])
            entry['candidate'] = len(self.submissions)+1
            self.submissions.append(entry)
            return entry
        if name == 'submit_selection':
            if len(self.workspace.candidates) + self.selection_count >= 6:
                raise Denied('candidate_budget_exhausted')
            for key, cap in (('name', 128), ('version', 128), ('evidence', 8192)):
                if (not isinstance(args[key], str) or not args[key]
                        or len(self.workspace._source(args[key], cap)) > cap
                        or any(ord(c) < 32 for c in args[key])):
                    raise Denied('invalid_selection')
            self.selection_count += 1
            self.submissions.append({'candidate': len(self.submissions)+1, 'kind': 'selection',
                                     'sha256': hashlib.sha256(encoded(args)).hexdigest(),
                                     'selection': copy.deepcopy(args)})
            self.selection = copy.deepcopy(args)
            self.workspace._check()
            self.workspace._new(self.workspace.artifact_fd, f'selection-{self.selection_count:02d}.json', encoded(args))
            return {'candidate': len(self.workspace.candidates)+self.selection_count, 'recorded': True}
        if name == 'read_catalog_facts':
            return copy.deepcopy(self.catalog_facts)
        if name == 'raw_validate':
            stage = args['stage']
            if stage not in ('check', 'fmt', 'clippy', 'test', 'metadata', 'quality'):
                raise Denied('invalid_stage')
            return self._validation(name, lambda record: self._raw(stage, cancel, record))
        if name == 'raw_explain':
            if not isinstance(args['code'], str) or not re.fullmatch(r'E[0-9]{4}', args['code']):
                raise Denied('invalid_diagnostic_code')
            return self._validation(name, lambda _: self.driver.request(
                {'op': 'execute', 'files': self.workspace.snapshot(), 'command': 'explain', 'code': args['code']}, cancel))
        if name == 'resource_read':
            if not isinstance(args['uri'], str) or not re.fullmatch(r'rust-artifact://prj_[0-9a-f]{32}/art_[0-9a-f]{32}', args['uri']):
                raise Denied('invalid_resource_uri')
            return self.driver.request({'op': 'resource', 'uri': args['uri']}, cancel)
        original = self.names[name]
        if original == 'rust.test' and (type(args.get('timeout', 30)) is not int or not 1 <= args.get('timeout', 30) <= 30):
            raise Denied('protocol_test_timeout_maximum_30')
        if original == 'rust.clippy' and self.strict_clippy and args.get('lint_profile') != 'strict':
            raise Denied('protocol_requires_explicit_strict_clippy')
        self.workspace.snapshot()
        request = {'op': 'call', 'name': original, 'arguments': copy.deepcopy(args)}
        operation = lambda _: self.driver.request(request, cancel)
        return self._validation(original, operation) if original in VALIDATIONS else operation(None)

    def receipt(self):
        return copy.deepcopy({'arm': self.arm, 'admitted_requests': min(64, self.calls), 'observed_requests': self.calls,
                              'validation_requests': self.validations,
                              'candidates': self.submissions,
                              'selection_count': self.selection_count,
                              'final_selection': self.selection, 'requests': self.events})
