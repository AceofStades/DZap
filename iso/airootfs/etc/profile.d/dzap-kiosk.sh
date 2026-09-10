#!/usr/bin/env bash

if [[ $(id -un) == "dzap" && $(tty) == "/dev/tty1" && -z ${DISPLAY:-} ]]; then
    exec startx /usr/local/bin/dzap-kiosk -- :0 vt1 -keeptty -nolisten tcp
fi
