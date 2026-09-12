#!/usr/bin/env bash

if [[ $(id -un) == "dzap" && -z ${DISPLAY:-} ]]; then
    case $(tty) in
        /dev/tty1)
            exec startx /usr/local/bin/dzap-kiosk -- :0 vt1 -keeptty -nolisten tcp
            ;;
        /dev/tty2)
            exec /usr/local/bin/dzap-tui
            ;;
    esac
fi
