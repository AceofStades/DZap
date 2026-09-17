#!/usr/bin/env python3
"""QEMU serial-console driver for the DZap e2e test.

Boots the Alpine live ISO, logs in as root (no password on live console),
pulls the backend binary + guest test script from a host HTTP server
(reachable from the guest as 10.0.2.2 via QEMU user-mode networking), and
runs the test. All guest output is mirrored to the serial log file.

Usage: e2e-driver.py <alpine.iso> <scratch.qcow2> <recovery-target.qcow2> <serial.log>
"""
import sys

import pexpect

ISO, SCRATCH, RECOVERY_TARGET, LOG = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4]

qemu_cmd = [
    "qemu-system-x86_64",
    "-machine", "q35,accel=kvm:tcg",
    "-cpu", "max",
    "-smp", "2",
    "-m", "1024",
    "-nographic",
    "-cdrom", ISO,
    "-boot", "d",
    "-drive", f"file={SCRATCH},if=virtio,format=qcow2",
    "-device", "qemu-xhci",
    "-drive", f"file={RECOVERY_TARGET},if=none,format=qcow2,id=recovery-target",
    "-device", "usb-storage,drive=recovery-target,removable=on",
    "-netdev", "user,id=n0",
    "-device", "virtio-net-pci,netdev=n0",
    "-no-reboot",
]

PROMPT = "DZAP# "


def run(child, cmd, timeout=120):
    """Send a command, require a zero exit status, and wait for the prompt."""
    child.sendline(
        f"{cmd}; dzap_command_status=$?; "
        'printf "\\n__DZAP_COMMAND_STATUS_%s\\n" "$dzap_command_status"'
    )
    child.expect(r"__DZAP_COMMAND_STATUS_([0-9]+)", timeout=timeout)
    status = int(child.match.group(1))
    child.expect(PROMPT, timeout=timeout)
    if status != 0:
        raise RuntimeError(f"guest command failed with status {status}: {cmd}")


def main():
    with open(LOG, "w") as logfile:
        child = pexpect.spawn(qemu_cmd[0], qemu_cmd[1:], encoding="utf-8", timeout=300)
        child.logfile = logfile
        try:
            child.expect("login:", timeout=300)
            child.sendline("root")
            child.expect("#", timeout=60)

            run(child, f"export PS1='{PROMPT}'")

            # Network up (QEMU slirp: DHCP on eth0, host reachable at 10.0.2.2).
            run(child, "ifconfig eth0 up && udhcpc -i eth0 -q")

            # Pull in the storage tools used by wipe and recovery execution.
            run(child, "setup-apkrepos -1", timeout=180)
            run(
                child,
                "sed -i '/\\/main$/p; s#/main$#/community#' /etc/apk/repositories && apk update",
                timeout=180,
            )
            run(
                child,
                "apk add util-linux e2fsprogs jq ddrescue testdisk cryptsetup",
                timeout=240,
            )

            # Fetch the static backend binary and the guest test script.
            run(child, "wget -q http://10.0.2.2:8123/server -O /root/server && chmod +x /root/server")
            run(child, "wget -q http://10.0.2.2:8123/guest-test.sh -O /root/guest-test.sh")

            # Run the test suite inside the guest. It prints E2E: markers and
            # powers the VM off when done.
            child.sendline("sh /root/guest-test.sh")
            idx = child.expect(
                [r"E2E: ALL TESTS PASSED", r"E2E: FAIL", pexpect.EOF, pexpect.TIMEOUT],
                timeout=600,
            )
            # Keep draining until the VM powers off so the serial log
            # captures the full failure output (diagnostics, server log).
            try:
                child.expect(pexpect.EOF, timeout=90)
            except pexpect.TIMEOUT:
                pass
            if idx != 0:
                logfile.write(f"\nDRIVER: guest did not report success (match index {idx})\n")
                return 1
            return 0
        finally:
            child.close(force=True)


if __name__ == "__main__":
    sys.exit(main())
