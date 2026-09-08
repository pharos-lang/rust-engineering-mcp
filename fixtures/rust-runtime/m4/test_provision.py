#!/usr/bin/env python3
import hashlib, importlib.util, io, json, tarfile, tempfile, unittest
from pathlib import Path
from unittest import mock
P=Path(__file__).with_name("provision.py"); S=importlib.util.spec_from_file_location("m4p",P); assert S and S.loader
m=importlib.util.module_from_spec(S); S.loader.exec_module(m)
def make(path,members):
    with tarfile.open(path,"w:gz") as tf:
        for name,data,kind in members:
            info=tarfile.TarInfo(name)
            if kind=="file": info.size=len(data); tf.addfile(info,io.BytesIO(data))
            else: info.type=tarfile.SYMTYPE; info.linkname=data.decode(); tf.addfile(info)
class Tests(unittest.TestCase):
    def test_manifest_closure(self):
        xs=m.inputs(json.loads(m.MANIFEST.read_text())); self.assertEqual(sum(x["kind"]=="registry-crate" for x in xs),238); self.assertEqual(len(xs),len({x["filename"] for x in xs}))
    def test_integrity_rejected(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/"x"; p.write_bytes(b"bad")
            with self.assertRaisesRegex(ValueError,"integrity mismatch"): m.download("https://static.crates.io/x",p,"0"*64,3)
    def test_download_checked(self):
        data=b"bytes"; h=hashlib.sha256(data).hexdigest(); response=mock.MagicMock(); response.__enter__.return_value=io.BytesIO(data); response.__exit__.return_value=False
        with tempfile.TemporaryDirectory() as d, mock.patch.object(m.urllib.request,"urlopen",return_value=response): self.assertEqual(m.download("https://static.crates.io/x",Path(d)/"x",h,len(data))["sha256"],h)
    def test_archive_traversal_and_link_rejected(self):
        with tempfile.TemporaryDirectory() as d:
            a=Path(d)/"a.crate"; make(a,[("../x",b"x","file")])
            with self.assertRaisesRegex(ValueError,"unsafe archive"): m.validate_archive(a)
            b=Path(d)/"b.crate"; make(b,[("x",b"/etc/passwd","link")])
            with self.assertRaisesRegex(ValueError,"linked or special"): m.validate_archive(b)
    def test_duplicate_rejected(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/"x.crate"; make(p,[("x",b"1","file"),("x",b"2","file")])
            with self.assertRaisesRegex(ValueError,"duplicate"): m.validate_archive(p)
    def test_context_extra_and_symlink_rejected(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d); (p/"x").write_text("x")
            with self.assertRaisesRegex(ValueError,"unexpected or linked"): m.validate_context(p,{"ok"})
        with tempfile.TemporaryDirectory() as d:
            p=Path(d); (p/"target").write_text("x"); (p/"ok").symlink_to(p/"target")
            with self.assertRaisesRegex(ValueError,"unexpected or linked"): m.validate_context(p,{"ok","target"})
    def test_packaged_lock_exact_path(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/"x.crate"; make(p,[("cargo-deny-0.19.7/Cargo.lock",b"lock","file")]); self.assertEqual(m.packaged_lock(p),b"lock")
            q=Path(d)/"y.crate"; make(q,[("other/Cargo.lock",b"lock","file")])
            with self.assertRaisesRegex(ValueError,"exact Cargo.lock"): m.packaged_lock(q)
if __name__=="__main__": unittest.main()
