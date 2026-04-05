# Remote Coast Testing Guide (From Scratch)

This guide shows a beginner-friendly, end-to-end workflow to test Remote Coast with a VM.

It covers:
- Initial setup
- Connecting local `coast-dev` to a remote VM
- Syncing your project files
- Building and running on the remote VM
- Running bash commands remotely with `coast-dev exec`

---

## 1) What Remote Coast does

When remote mode is enabled for a project:
- Your local `coast-dev` daemon stays in control.
- It opens an SSH tunnel to the VM.
- It syncs project files to the VM with Mutagen.
- `coast-dev build` and `coast-dev run` execute on the VM.
- `coast-dev exec` runs commands inside containers that are running on the VM.

So you use local commands, but execution happens remotely.

---

## 2) Prerequisites

On your local machine:
- `coast-dev` built and available
- `mutagen` installed
- SSH access to VM (user, host, key)

On the VM:
- Ubuntu/Linux with Docker installed
- Enough disk space (recommended 10+ GB free)
- SSH reachable from local machine

Quick checks:

```bash
coast-dev daemon status
mutagen version
ssh -i ~/.ssh/coast_vm_key ubuntu@<VM_IP> "echo ok"
ssh -i ~/.ssh/coast_vm_key ubuntu@<VM_IP> "docker --version"
```

---

## 3) Start local dev daemon

```bash
coast-dev daemon start
coast-dev daemon status
```

If already running, that is fine.

---

## 4) Add, setup, and connect remote VM

Replace values with your VM details.

```bash
# Add remote definition
coast-dev remote add myvm ubuntu@<VM_IP> --key ~/.ssh/coast_vm_key

# Install/setup remote coast daemon
coast-dev remote setup myvm

# Establish tunnel
coast-dev remote connect myvm

# Verify
coast-dev remote ls
coast-dev remote ping myvm
```

Expected: remote shows as connected with a local tunnel port.

---

## 5) Create file sync (local -> remote)

From your project directory (the one containing `Coastfile`):

```bash
cd /path/to/your/project

# Example: branch main
coast-dev sync create crm-demo myvm --local-path "$PWD" --branch main

# Confirm sync session
coast-dev sync status
mutagen sync list
```

Optional verification over SSH:

```bash
ssh -i ~/.ssh/coast_vm_key ubuntu@<VM_IP> "ls -la ~/coast-workspaces/crm-demo/main"
```

If your Coastfile project name differs from folder name, make sure the sync project name matches Coastfile project name.

---

## 6) Set project mode to remote

Current dev workflow may require setting project mode in the dev DB directly:

```bash
sqlite3 ~/.coast-dev/state.db "
INSERT INTO project_modes (project, mode, remote_name, updated_at)
VALUES ('crm-demo', 'remote', 'myvm', datetime('now'))
ON CONFLICT(project) DO UPDATE SET
  mode='remote',
  remote_name='myvm',
  updated_at=datetime('now');
"
```

Use your actual Coastfile project name instead of `crm-demo`.

---

## 7) Build and run remotely

From the project directory:

```bash
coast-dev build
coast-dev run test1
coast-dev ls
```

Expected:
- Build artifact created on VM
- Instance `test1` created and running
- Instance appears in local `coast-dev ls`

---

## 8) Execute bash commands on remote containers

This is the key test for remote command execution.

From the project directory:

```bash
# Run a simple command inside the instance
coast-dev exec test1 -- echo "hello from remote"

# Run shell command
coast-dev exec test1 -- sh -lc "pwd && ls -la"

# Example: check environment
coast-dev exec test1 -- env | head
```

Why this is remote:
- `test1` is tracked with `remote_name=myvm` in local state.
- Local daemon routes `exec` request through tunnel to remote daemon.
- Remote daemon executes command in the remote container.

Optional proof via SSH:

```bash
ssh -i ~/.ssh/coast_vm_key ubuntu@<VM_IP> "docker ps --format '{{.Names}}'"
```

You should see containers corresponding to your remote instance.

---

## 9) Logs, stop, remove

```bash
coast-dev logs test1
coast-dev stop test1
coast-dev rm test1
```

These also route to remote when the instance lives on a remote.

---

## 10) Troubleshooting

### A) `tunnel is not connected`

```bash
coast-dev remote connect myvm
coast-dev remote ls
```

### B) `Connection refused`

Check remote daemon and SSH:

```bash
ssh -i ~/.ssh/coast_vm_key ubuntu@<VM_IP> "pgrep -fa coastd"
coast-dev daemon logs
```

### C) `No such file or directory` for Coastfile on remote

Usually sync path/project mismatch:
- Coastfile project name and sync project name must match.
- Sync remote path must exist and contain your files.

```bash
coast-dev sync status
mutagen sync list
```

### D) VM out of disk space

```bash
ssh -i ~/.ssh/coast_vm_key ubuntu@<VM_IP> "df -h /"
ssh -i ~/.ssh/coast_vm_key ubuntu@<VM_IP> "docker system prune -af"
```

If needed, increase VM disk and expand filesystem.

---

## 11) Cleanup

```bash
coast-dev rm test1
coast-dev sync terminate crm-demo
coast-dev remote disconnect myvm
```

Optional full cleanup:

```bash
coast-dev remote remove myvm
```

---

## Success checklist

- Remote added/setup/connected
- Sync session active and files visible on VM
- `coast-dev build` succeeds in remote mode
- `coast-dev run` creates remote instance
- `coast-dev exec <instance> -- <cmd>` runs successfully
- `logs/stop/rm` work for that remote instance
