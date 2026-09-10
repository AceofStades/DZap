.PHONY: iso run-iso smoke-iso check-live

iso:
	./scripts/build-live-iso.sh

run-iso:
	./scripts/run-live-iso.sh

smoke-iso:
	./scripts/smoke-live-iso.py

check-live:
	bash -n scripts/build-live-iso.sh scripts/run-live-iso.sh
	bash -n iso/airootfs/etc/profile.d/dzap-kiosk.sh
	bash -n iso/airootfs/usr/local/bin/dzap-kiosk
	python -m py_compile scripts/smoke-live-iso.py
