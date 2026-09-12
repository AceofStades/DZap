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
	bash -n iso/airootfs/etc/profile.d/dzap-kiosk.sh
	bash -n iso/airootfs/usr/local/bin/dzap-kiosk
	test "$$(readlink iso/airootfs/etc/systemd/system/getty.target.wants/getty@tty2.service)" = /usr/lib/systemd/system/getty@.service
	python -m py_compile scripts/smoke-live-iso.py
