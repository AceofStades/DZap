.PHONY: build-iso iso secure-boot-key secure-iso verify-secure-iso run-iso run-tui smoke-iso check-live

SECURE_BOOT_DIR ?= $(CURDIR)/build/secure-boot
SECURE_ISO_OUT_DIR ?= $(CURDIR)/out/secure

build-iso:
	./scripts/build-live-iso.sh

iso: build-iso

$(SECURE_BOOT_DIR)/db.key:
	./scripts/generate-secure-boot-key.sh "$(SECURE_BOOT_DIR)"

secure-boot-key: $(SECURE_BOOT_DIR)/db.key

secure-iso: $(SECURE_BOOT_DIR)/db.key
	DZAP_SECURE_BOOT=1 \
	DZAP_SECURE_BOOT_DIR="$(SECURE_BOOT_DIR)" \
	DZAP_ISO_OUT_DIR="$(SECURE_ISO_OUT_DIR)" \
	./scripts/build-live-iso.sh

verify-secure-iso:
	./scripts/verify-secure-iso.py $(if $(ISO),"$(ISO)",) \
		--certificate "$(SECURE_BOOT_DIR)/db.pem"

run-iso:
	./scripts/run-live-iso.sh

run-tui:
	cd server && cargo run --features tui --bin dzap-tui

smoke-iso:
	./scripts/smoke-live-iso.py

check-live:
	bash -n scripts/build-live-iso.sh scripts/generate-secure-boot-key.sh scripts/run-live-iso.sh iso/secure-boot/make-uki.sh
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
	grep -qx 'efi    /%INSTALL_DIR%/boot/%ARCH%/vmlinuz-dzap.efi' iso/boot/efiboot/loader/entries/01-dzap-secure.conf
	python -m py_compile scripts/smoke-live-iso.py scripts/verify-secure-iso.py
