.PHONY: iso run-iso run-tui smoke-iso check-live

iso:
	./scripts/build-live-iso.sh

run-iso:
	./scripts/run-live-iso.sh

run-tui:
	cd server && cargo run --features tui --bin dzap-tui

smoke-iso:
	./scripts/smoke-live-iso.py

check-live:
	bash -n scripts/build-live-iso.sh scripts/run-live-iso.sh
	sh -n server/scripts/guest-test.sh
	bash -n iso/airootfs/etc/profile.d/dzap-kiosk.sh
	bash -n iso/airootfs/usr/local/bin/dzap-kiosk
	test "$$(readlink iso/airootfs/etc/systemd/system/getty.target.wants/getty@tty2.service)" = /usr/lib/systemd/system/getty@.service
	grep -qx 'DEFAULT dzap' iso/boot/syslinux/archiso_sys.cfg
	grep -qx 'TIMEOUT 1' iso/boot/syslinux/archiso_sys.cfg
	grep -qx 'default 01-dzap.conf' iso/boot/efiboot/loader/loader.conf
	grep -qx 'timeout 0' iso/boot/efiboot/loader/loader.conf
	! grep -Riq 'Arch Linux install medium' iso/boot
	grep -qx 'cryptsetup' iso/packages.x86_64
	grep -qx 'ddrescue' iso/packages.x86_64
	grep -qx 'testdisk' iso/packages.x86_64
	python -m py_compile scripts/smoke-live-iso.py
