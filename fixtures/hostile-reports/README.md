# Hostile quality-report corpus

These files are parser and egress adversaries. They are data only and must not
be opened in an active browser context or extracted outside a bounded staging
root.

| File | SHA-256 | Required trigger |
| --- | --- | --- |
| `junit-billion-laughs.xml` | `8ebe6d16800d862404ba9ea04097ca7300882326afe051a7f4a449cb02bad876` | Reject XML entity expansion/DTD; no network or filesystem resolution |
| `junit-external-entity.xml` | `1d4a14477415d53664e3f657e0d00acea7dc1501bf33fa72849d3897b4ab57ae` | Reject external entity and preserve safe error |
| `junit-deep-nesting.xml` | `a251d5f8579240462bdd76b3297247ce4cd148d3867e201c25901590ba8bd1ac` | Reject bounded-depth overflow without stack exhaustion |
| `junit-huge-attribute.xml` | `1f4f811ca89f4fb744458a61761a8b9ef4264606f0a3d1cd7ad8c730a756d6b5` | Reject input-size/attribute quota overflow |
| `junit-forged-markers.xml` | `75928eeef3bbbae7054127b934066356d3bc2abeb1d7947554715c5404f81a01` | Do not infer pass from embedded Cargo/nextest-looking text |
| `junit-nextest-sample-flaky-rerun-leak.xml` | `1e81a82bd2ef4c8d430c3e64bc76f2e8831e4308d4103c2e2504add28fcd41d9` | Parse retry and leak evidence as distinct outcomes |
| `coverage-deep.json` | `3db929a4dc6b1b0af06f354b74668f733c6ccece19f4d1cafbf375c37e5c7538` | Reject bounded JSON nesting overflow |
| `coverage-external-uri.html` | `88cd2ba920a4c50410795e333345979645e144a3b8b70df0975890af443b08d1` | Sanitize/disable external resources during preview |
| `report-with-script.html` | `183e6d74ffb5bcfc2861aa52994a3aee9a9db36473382d3467a34c104855e24a` | Sanitize scripts, event handlers and javascript URLs |
| `bundle-with-symlink.tar` | `81f95c53668b49e10032aac30d6bfd2e7fc06415e07574ae8b4b4aa3816449ae` | Reject symlink members and never follow the link |
| `bundle-with-dotdot.tar` | `52157df5f066948a6973cd02e37c273fedf5d935d53edb14053413d628a6bbca` | Reject traversal member names |
| `bundle-oversize-member.tar` | `1de22ae3aa0c09abf8b8d18c007116fe0d3305e588bbfba6bc980635d2a0de88` | Reject member/job byte quota before extraction |

The three archives use deterministic USTAR headers and `mtime=1700000000`.
