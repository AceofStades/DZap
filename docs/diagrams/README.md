# Shareable system diagrams

These standalone diagrams describe the current DZap bootable-USB design. Open an HTML file directly in a modern browser; it contains its own renderer, theme switcher, views, and export controls and does not contact a server.

| Diagram | Audience | Editable source |
| --- | --- | --- |
| [System architecture](dzap-system.html) | Boot trust chain, live runtime, privilege boundaries, storage operations, and evidence flow. | [`dzap-system.architecture.json`](dzap-system.architecture.json) |
| [Wipe workflow](wipe-workflow.html) | Identity-bound preflight, sanitization, mandatory verification, hash-chained evidence, certificates, and export. | [`wipe-workflow.workflow.json`](wipe-workflow.workflow.json) |
| [Recovery workflow](recovery-workflow.html) | Read-only assessment, separate destination, resumable ddrescue imaging, image inspection, extraction, and manifests. | [`recovery-workflow.workflow.json`](recovery-workflow.workflow.json) |

The architecture diagram calls out the current Secure Boot limit: firmware and systemd-boot authenticate the signed UKI, while the external ArchISO SquashFS remains outside that signature.

All three sources pass Archify schema validation and the 9/9 showcase checklist. Their standalone artifacts were visually checked in light and dark themes at 1440×900, 1600×1000, 1920×1080, and 2048×1320 with no viewport overflow.

Regenerate a diagram from the repository's installed Archify skill directory:

```bash
node bin/archify.mjs validate architecture /absolute/path/to/system.architecture.json --quality showcase
node bin/archify.mjs deliver architecture /absolute/path/to/system.architecture.json /absolute/path/to/system.html --quality showcase

node bin/archify.mjs validate workflow /absolute/path/to/process.workflow.json --quality showcase
node bin/archify.mjs deliver workflow /absolute/path/to/process.workflow.json /absolute/path/to/process.html --quality showcase
```

Commit both the JSON source and its rendered HTML so teammates can review the model and open the presentation without installing Archify.
