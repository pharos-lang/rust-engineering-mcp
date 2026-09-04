"""Read-only fixed Docker observations outside participant feedback; no cleanup mutations."""
import json
import os
from pathlib import Path
import selectors
import subprocess
import time

DOCKER = Path('/Applications/Docker.app/Contents/Resources/bin/docker')
LIMIT = 65536


def observe(socket, private_parent):
    parent = Path(private_parent)
    config = parent/'docker-observer'
    config.mkdir(mode=0o700, exist_ok=True)
    path = config/'config.json'
    if not path.exists():
        fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
        try: os.write(fd, b'{}\n')
        finally: os.close(fd)
    if path.is_symlink() or path.read_bytes() != b'{}\n':
        raise ValueError('observer_config_changed')
    receipt = {'mode':'read_only_observation_no_cleanup','objects':{},'absent':False}
    for kind in ('container','volume'):
        args = [str(DOCKER),'--config',str(config),'--host','unix://'+str(socket),kind,'ls']
        if kind == 'container': args += ['--all']
        args += ['--filter','label=org.rust-mcp.execution=true','--format','{{.ID}}' if kind=='container' else '{{.Name}}']
        child = subprocess.Popen(args,env={},stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        select = selectors.DefaultSelector(); data={'stdout':bytearray(),'stderr':bytearray()};deadline=time.monotonic()+15
        try:
            for name,stream in [('stdout',child.stdout),('stderr',child.stderr)]:
                os.set_blocking(stream.fileno(),False);select.register(stream,selectors.EVENT_READ,name)
            while select.get_map() or child.poll() is None:
                if time.monotonic()>=deadline: raise ValueError('observer_deadline')
                for key,_ in select.select(.05):
                    chunk=os.read(key.fileobj.fileno(),8192)
                    if not chunk: select.unregister(key.fileobj);continue
                    data[key.data].extend(chunk)
                    if len(data[key.data])>LIMIT:raise ValueError('observer_output_limit')
            if child.wait()!=0 or data['stderr']:raise ValueError('observer_command_failed')
            receipt['objects'][kind]=bytes(data['stdout']).decode('utf-8').splitlines()
        finally:
            if child.poll() is None:child.kill()
            child.wait(timeout=5);select.close();child.stdout.close();child.stderr.close()
    receipt['absent']=all(not rows for rows in receipt['objects'].values())
    return receipt
