//go:build linux

// execution-probe is a fixed-scenario Linux sandbox test fixture. It must run
// only inside the test container: it is deliberately not a general command runner.
package main

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"runtime"
	"sort"
	"strings"
	"sync"
	"syscall"
	"time"
)

const maxLifetime = 60 * time.Second

var scenario string

type record struct {
	Scenario string `json:"scenario"`
	Event    string `json:"event"`
	PID      int    `json:"pid"`
	Details  any    `json:"details"`
}

type outcome struct {
	Allowed bool   `json:"allowed"`
	Errno   int    `json:"errno"`
	Error   string `json:"error"`
}

func result(err error) outcome {
	if err == nil {
		return outcome{Allowed: true}
	}
	var errno syscall.Errno
	if errors.As(err, &errno) {
		return outcome{Errno: int(errno), Error: errno.Error()}
	}
	return outcome{Error: err.Error()}
}

func emitTo(w io.Writer, event string, details any) {
	if err := json.NewEncoder(w).Encode(record{scenario, event, os.Getpid(), details}); err != nil {
		// A closed/full output channel is part of gateway cancellation. Do not
		// recursively log or wait after the stream becomes unavailable.
		os.Exit(74)
	}
}

func emit(event string, details any) { emitTo(os.Stdout, event, details) }

func main() {
	if len(os.Args) != 2 {
		scenario = "invalid"
		emit("rejected", map[string]string{"reason": "exactly one scenario argument is required"})
		os.Exit(64)
	}
	scenario = os.Args[1]
	switch scenario {
	case "success":
		emit("completed", map[string]bool{"ok": true})
	case "exit7":
		emit("completed", map[string]int{"exit_code": 7})
		os.Exit(7)
	case "output":
		output()
	case "sleep":
		emit("started", map[string]int{"duration_seconds": 60})
		time.Sleep(maxLifetime)
		emit("completed", nil)
	case "environment":
		environment := os.Environ()
		sort.Strings(environment)
		emit("environment", map[string]any{"entries": environment})
	case "network":
		network()
	case "filesystem":
		filesystem()
	case "descendants":
		descendants()
	case "daemonize": // Fixed intermediate process; creates the orphan fixture.
		daemonize()
	case "heartbeat": // Internal descendant; fixed behavior, no extra arguments.
		heartbeat()
	case "pids":
		pids()
	case "memory":
		memory()
	case "disk":
		disk()
	case "cpu":
		cpu()
	case "cgroups":
		emit("cgroups", cgroups())
	default:
		emit("rejected", map[string]string{"reason": "scenario is not allowlisted"})
		os.Exit(64)
	}
}

func output() {
	var writers sync.WaitGroup
	for _, stream := range []struct {
		name string
		file *os.File
	}{{"stdout", os.Stdout}, {"stderr", os.Stderr}} {
		writers.Add(1)
		go func(name string, file *os.File) {
			defer writers.Done()
			payload := strings.Repeat("x", 65536)
			for index := 0; index < 20; index++ {
				emitTo(file, "output", map[string]any{"stream": name, "index": index, "payload": payload})
			}
		}(stream.name, stream.file)
	}
	writers.Wait()
	emit("completed", map[string]int{"records_per_stream": 20, "payload_bytes_per_record": 65536})
}

func network() {
	// No connect, bind, sendto, resolver, DNS lookup, or external traffic occurs.
	// All cases measure only the sandbox's socket-creation policy. A denied
	// socket makes both loopback and DNS traffic impossible through that API.
	for _, family := range []struct {
		name   string
		domain int
	}{{"ipv4", syscall.AF_INET}, {"ipv6", syscall.AF_INET6}} {
		for _, transport := range []struct {
			name     string
			kind     int
			protocol int
		}{{"tcp", syscall.SOCK_STREAM, syscall.IPPROTO_TCP}, {"udp", syscall.SOCK_DGRAM, syscall.IPPROTO_UDP}} {
			for _, purpose := range []string{"loopback", "dns"} {
				fd, err := syscall.Socket(family.domain, transport.kind|syscall.SOCK_CLOEXEC, transport.protocol)
				observed := result(err)
				if err == nil {
					_ = syscall.Close(fd)
				}
				emit("socket", map[string]any{"family": family.name, "transport": transport.name, "purpose": purpose, "operation": "socket_only", "result": observed})
			}
		}
	}
	// Two additional local socket families exercise non-IP IPC surfaces.
	// Neither socket is bound, connected, or used to exchange messages.
	for _, local := range []struct {
		family    string
		transport string
		domain    int
		kind      int
		protocol  int
	}{
		{"unix", "stream", syscall.AF_UNIX, syscall.SOCK_STREAM, 0},
		{"netlink", "raw", syscall.AF_NETLINK, syscall.SOCK_RAW, syscall.NETLINK_ROUTE},
	} {
		fd, err := syscall.Socket(local.domain, local.kind|syscall.SOCK_CLOEXEC, local.protocol)
		observed := result(err)
		if err == nil {
			_ = syscall.Close(fd)
		}
		emit("socket", map[string]any{"family": local.family, "transport": local.transport, "purpose": "local", "operation": "socket_only", "result": observed})
	}
}

func attemptWrite(path string, create bool) outcome {
	flags := os.O_WRONLY | os.O_APPEND
	if create {
		flags |= os.O_CREATE
	}
	file, err := os.OpenFile(path, flags, 0600)
	if err != nil {
		return result(err)
	}
	_, writeErr := file.Write([]byte("execution-probe\n"))
	closeErr := file.Close()
	if writeErr != nil {
		return result(writeErr)
	}
	return result(closeErr)
}

func readCanary() ([]byte, error) {
	file, err := os.Open("/rootfs-canary")
	if err != nil {
		return nil, err
	}
	defer file.Close()
	data, err := io.ReadAll(io.LimitReader(file, 4097))
	if len(data) > 4096 {
		return nil, fmt.Errorf("canary exceeds 4096 bytes")
	}
	return data, err
}

type swapResult struct {
	Swaps                int `json:"swaps"`
	Attempts             int `json:"attempts"`
	PositiveWrites       int `json:"positive_writes"`
	ReadOnlyDenials      int `json:"readonly_denials"`
	UnexpectedRootWrites int `json:"unexpected_root_writes"`
	OtherErrors          int `json:"other_errors"`
}

func symlinkSwap(rootInfo os.FileInfo) swapResult {
	// Atomic rename swaps a fixed symlink while the writer opens it. File
	// identity is checked on the opened handle, never guessed from readlink.
	// Every operation is finite and confined to fixed fixture paths.
	type swaps struct{ count, errors int }
	finished := make(chan swaps, 1)
	go func() {
		result := swaps{}
		for index := 0; index < 256; index++ {
			target := "/work/positive"
			if index%2 == 0 {
				target = "/rootfs-canary"
			}
			if err := os.Symlink(target, "/work/probe-link-next"); err != nil {
				result.errors++
				break
			}
			if err := os.Rename("/work/probe-link-next", "/work/probe-link"); err != nil {
				result.errors++
				_ = os.Remove("/work/probe-link-next")
				break
			}
			result.count++
			runtime.Gosched()
		}
		finished <- result
	}()
	observed := swapResult{}
	for index := 0; index < 512; index++ {
		observed.Attempts++
		file, err := os.OpenFile("/work/probe-link", os.O_WRONLY|os.O_APPEND, 0)
		if err != nil {
			if errors.Is(err, syscall.EROFS) {
				observed.ReadOnlyDenials++
			} else {
				observed.OtherErrors++
			}
			runtime.Gosched()
			continue
		}
		info, statErr := file.Stat()
		if statErr != nil {
			observed.OtherErrors++
			_ = file.Close()
			continue
		}
		_, writeErr := file.Write([]byte("swap-probe\n"))
		closeErr := file.Close()
		if writeErr == nil {
			if os.SameFile(rootInfo, info) {
				observed.UnexpectedRootWrites++
			} else {
				observed.PositiveWrites++
			}
		} else if errors.Is(writeErr, syscall.EROFS) {
			observed.ReadOnlyDenials++
		} else {
			observed.OtherErrors++
		}
		if closeErr != nil {
			observed.OtherErrors++
		}
		runtime.Gosched()
	}
	completed := <-finished
	observed.Swaps = completed.count
	observed.OtherErrors += completed.errors
	return observed
}

func filesystem() {
	before, beforeErr := readCanary()
	rootInfo, statErr := os.Stat("/rootfs-canary")
	worldWritable := statErr == nil && rootInfo.Mode().Perm()&0002 != 0
	emit("rootfs_canary", map[string]any{"path": "/rootfs-canary", "world_writable": worldWritable, "read_result": result(beforeErr), "stat_result": result(statErr)})
	binary := attemptWrite("/mcp-probe", false)
	emit("write", map[string]any{"path": "/mcp-probe", "result": binary})
	root := attemptWrite("/rootfs-canary", false)
	emit("write", map[string]any{"path": "/rootfs-canary", "result": root})
	positive := attemptWrite("/work/positive", true)
	emit("write", map[string]any{"path": "/work/positive", "result": positive})
	_, err := os.Stat("/etc/host-canary")
	absent := errors.Is(err, os.ErrNotExist)
	emit("host_canary", map[string]any{"path": "/etc/host-canary", "result": result(err), "absent": absent})
	err = os.Symlink("/rootfs-canary", "/work/probe-link")
	emit("symlink", map[string]any{"path": "/work/probe-link", "target": "/rootfs-canary", "result": result(err)})
	symlinkDenied := false
	observed := swapResult{}
	if err == nil {
		link := attemptWrite("/work/probe-link", false)
		symlinkDenied = !link.Allowed && link.Errno == int(syscall.EROFS)
		emit("symlink_write", map[string]any{"path": "/work/probe-link", "result": link})
		if statErr == nil {
			observed = symlinkSwap(rootInfo)
		}
	}
	emit("symlink_swap", observed)
	after, afterErr := readCanary()
	unchanged := beforeErr == nil && afterErr == nil && bytes.Equal(before, after)
	passed := worldWritable && !binary.Allowed && !root.Allowed && root.Errno == int(syscall.EROFS) && positive.Allowed && absent && symlinkDenied && observed.Swaps == 256 && observed.Attempts == 512 && observed.UnexpectedRootWrites == 0 && observed.OtherErrors == 0 && unchanged
	emit("filesystem_assertions", map[string]any{"passed": passed, "rootfs_unchanged": unchanged, "canary_read_result": result(afterErr), "unexpected_root_writes": observed.UnexpectedRootWrites})
	if !passed {
		os.Exit(1)
	}
}

func child(name string) *exec.Cmd {
	command := exec.Command("/mcp-probe", name)
	command.Env = []string{"GOMAXPROCS=1"}
	command.Stdin = nil
	return command
}

func stop(command *exec.Cmd) {
	if command.Process != nil {
		_ = command.Process.Kill()
		_ = command.Wait()
	}
}

func descendants() {
	command := child("daemonize")
	command.Stdout = os.Stdout
	command.Stderr = os.Stderr
	if err := command.Start(); err != nil {
		emit("spawn_failed", result(err))
		os.Exit(70)
	}
	// Waiting for the intermediate makes its exit and the orphan transition
	// deterministic before the parent starts its bounded lifetime wait.
	if err := command.Wait(); err != nil {
		emit("intermediate_failed", result(err))
		os.Exit(70)
	}
	emit("intermediate_exited", map[string]any{"intermediate_pid": command.Process.Pid, "parent_process_group": syscall.Getpgrp(), "duration_seconds": 60})
	time.Sleep(maxLifetime)
	emit("completed", nil)
}

func daemonize() {
	command := child("heartbeat")
	command.SysProcAttr = &syscall.SysProcAttr{Setsid: true}
	command.Stdout = os.Stdout
	command.Stderr = os.Stderr
	if err := command.Start(); err != nil {
		emit("spawn_failed", result(err))
		os.Exit(70)
	}
	// The intermediate deliberately does not wait or kill the grandchild.
	// It exits after reporting its identity; Linux reparents the grandchild
	// to PID 1 (the fixture parent or an init process in the PID namespace).
	scenario = "descendants"
	emit("descendant_started", map[string]any{"child_pid": command.Process.Pid, "intermediate_pid": os.Getpid(), "original_parent_pid": os.Getppid(), "parent_process_group": syscall.Getpgrp(), "setsid": true, "double_fork": true, "duration_seconds": 60})
	if err := command.Process.Release(); err != nil {
		emit("release_failed", result(err))
		os.Exit(70)
	}
}

func heartbeat() {
	deadline := time.Now().Add(maxLifetime)
	for tick := 0; time.Now().Before(deadline); tick++ {
		// Getpgrp is the child's PID after Setsid; session creation itself is
		// enforced by exec.Cmd before the child enters main.
		emit("heartbeat", map[string]int{"tick": tick, "parent_pid": os.Getppid(), "process_group": syscall.Getpgrp()})
		time.Sleep(100 * time.Millisecond)
	}
}

func pids() {
	commands := make([]*exec.Cmd, 0, 80)
	defer func() {
		for _, command := range commands {
			stop(command)
		}
	}()
	observedEAGAIN := false
	for attempt := 0; attempt < 80; attempt++ {
		command := child("sleep")
		command.Stdout = io.Discard
		command.Stderr = io.Discard
		err := command.Start()
		if err != nil {
			observedEAGAIN = errors.Is(err, syscall.EAGAIN)
			emit("spawn_failed", map[string]any{"attempt": attempt, "result": result(err), "eagain": observedEAGAIN})
			break
		}
		commands = append(commands, command)
	}
	emit("pids", map[string]any{"started": len(commands), "maximum_attempts": 80, "eagain": observedEAGAIN, "cgroups": cgroups()})
}

func memory() {
	chunks := make([][]byte, 0, 24)
	emit("memory_started", map[string]int{"maximum_bytes": 192 * 1024 * 1024})
	for count := 1; count <= 24; count++ {
		chunk := make([]byte, 8*1024*1024)
		for offset := 0; offset < len(chunk); offset += 4096 {
			chunk[offset] = byte(count)
		}
		chunks = append(chunks, chunk)
		emit("memory_allocated", map[string]int{"bytes": count * 8 * 1024 * 1024})
	}
	time.Sleep(maxLifetime)
	runtime.KeepAlive(chunks)
	emit("completed", nil)
}

func disk() {
	file, err := os.OpenFile("/work/disk-probe", os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
	if err != nil {
		emit("disk", map[string]any{"bytes_written": 0, "result": result(err), "enospc": errors.Is(err, syscall.ENOSPC)})
		return
	}
	defer file.Close()
	block := make([]byte, 64*1024)
	written := 0
	for written < 32*1024*1024 {
		count, writeErr := file.Write(block)
		written += count
		if writeErr != nil {
			emit("disk", map[string]any{"bytes_written": written, "result": result(writeErr), "enospc": errors.Is(writeErr, syscall.ENOSPC)})
			return
		}
		if count == 0 {
			emit("disk", map[string]any{"bytes_written": written, "result": result(io.ErrShortWrite), "enospc": false})
			return
		}
	}
	emit("disk", map[string]any{"bytes_written": written, "result": result(nil), "enospc": false})
}

func readCgroup(name string) map[string]any {
	// name comes exclusively from fixed internal literals below.
	file, err := os.Open("/sys/fs/cgroup/" + name)
	if err != nil {
		return map[string]any{"value": "", "result": result(err)}
	}
	defer file.Close()
	data, err := io.ReadAll(io.LimitReader(file, 4097))
	if len(data) > 4096 {
		err = fmt.Errorf("cgroup file exceeds 4096 bytes")
	}
	return map[string]any{"value": strings.TrimSpace(string(data)), "result": result(err)}
}

func cgroups() map[string]any {
	return map[string]any{
		"pids.max": readCgroup("pids.max"), "pids.current": readCgroup("pids.current"),
		"pids.events": readCgroup("pids.events"), "memory.max": readCgroup("memory.max"),
		"memory.events": readCgroup("memory.events"), "cpu.max": readCgroup("cpu.max"),
	}
}

func cpu() {
	before := readCgroup("cpu.stat")
	maximum := readCgroup("cpu.max")
	started := time.Now()
	deadline := started.Add(3 * time.Second)
	var iterations uint64
	var checksum uint64 = 1
	for time.Now().Before(deadline) {
		for count := 0; count < 65536; count++ {
			checksum = checksum*6364136223846793005 + 1442695040888963407
		}
		iterations++
	}
	emit("cpu", map[string]any{"before": before, "after": readCgroup("cpu.stat"), "cpu.max": maximum, "elapsed_milliseconds": time.Since(started).Milliseconds(), "iterations": iterations, "checksum": checksum})
}
