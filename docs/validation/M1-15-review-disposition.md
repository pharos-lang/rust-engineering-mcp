# M1-15 principal disposition

No High/Critical finding. Reviewed scripts, source manifest, installation, archive
receipts and real active doctor reports against the actual local artifacts.

- Low provenance readability: accepted. Build receipt intentionally preserves the
  actual pre-commit HEAD and dirty state; it must be read with accepted-source-receipt,
  which compares the exact238 inputs against merged main01a90ab6 and working tree.
  Added an explicit caveat; never rewrite the historical build observation.
- Low boolean-precedence readability: understood; expressions implement the correct
  grouping and produced payloads verified byte-by-byte. Exact executed scripts are
  preserved for reproduction rather than changed after measurement. No runtime defect.
- Reviewer's praise of symlink rejection is limited: prechecks do not establish
  TOCTOU safety against concurrent hostile writers. That threat is outside these
  controlled developer harnesses; no such guarantee is claimed for them. Product
  import keeps its separate handle-relative authenticated boundary.

Local metadata check and git diff check pass. Two actual release active doctors
passed after the private installation checks; Docker objects were absent before
and after. Pending licenses/publisher/native hosts remain explicit. M1-15 is still
In progress pending those external inputs and complete release qualification.
