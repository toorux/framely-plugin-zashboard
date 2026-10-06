# Third-party software and data

zashboard plugin code and the bundled Framely SDK are distributed under AGPL-3.0-only; see LICENSE and LICENSE.framely-sdk. The independent Mihomo executable is GPL-3.0; see LICENSE.mihomo. zashboard is MIT; see LICENSE.zashboard. React and React DOM are MIT; their license files are included.

Pinned versions and checksums are recorded in upstream.lock.json:

- Mihomo v1.19.32: https://github.com/MetaCubeX/mihomo/tree/v1.19.32
- zashboard v3.29.1: https://github.com/Zephyruso/zashboard/tree/v3.29.1
- Framely SDK 0.4.0, copied from the local Framely project: https://github.com/toorux/framely

Corresponding upstream source archives are bundled under upstream/. The source.zip archive contains the plugin implementation, build scripts, dependency lockfiles and the complete SDK used to build its page bundles. zashboard's unmodified dependency notices and font licenses remain in the dashboard directory. The only dashboard changes are removal of the service-worker registration tag and addition of the Framely keyboard/hover bridge script; scripts/prepare.py documents these changes.

Offline geoip.dat, geosite.dat and Country.mmdb are snapshots from https://github.com/MetaCubeX/meta-rules-dat. SHA256 values in upstream.lock.json identify the exact snapshots, and their download URLs are pinned to an immutable release-branch commit. The project's README and data-source attribution are bundled as GEO_DATA_NOTICES.md. These generated datasets incorporate separately licensed upstream lists and geolocation data; their original terms continue to apply. MetaCubeX does not endorse this plugin.

The application names and upstream branding belong to their respective projects. The zashboard icon is a new code-drawn plugin asset.
