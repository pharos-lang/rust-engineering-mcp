#!/usr/bin/env python3
"""Validate and summarize the exact 30 cold/30 warm M4 operation observations."""
import hashlib
import json
import math
import pathlib
import statistics

ROOT=pathlib.Path(__file__).resolve().parents[1]
TOOLS={"rust.deny","rust.unsafe.scan","rust.miri","rust.supply_chain.inspect","rust.quality.gate.v2"}

def main():
    measured=ROOT/"target/m4-budgets.json"
    inputs=ROOT/"target/m4-budgets-inputs.json"
    receipt=json.loads(measured.read_text()); source=json.loads(inputs.read_text())
    assert receipt["status"]=="passed" and receipt["samples_each"]==30
    assert receipt["binary_sha256"]=="sha256:"+source["binary_sha256"]
    rows=receipt["measurements"];assert len(rows)==300
    assert {row["tool"] for row in rows}==TOOLS
    groups=[]
    for tool in sorted(TOOLS):
        for temperature in ["cold","warm"]:
            selected=[r for r in rows if r["tool"]==tool and r["temperature"]==temperature]
            assert len(selected)==30 and sorted(r["sample"] for r in selected)==list(range(30))
            values=sorted(row["elapsed_ms"] for row in selected)
            assert values[0]>0 and values[-1]<=60_000
            assert all(0<row["reply_bytes"]<=512*1024 for row in selected)
            groups.append({"tool":tool,"temperature":temperature,"samples":30,"min_ms":values[0],"median_ms":statistics.median(values),"p95_ms":values[math.ceil(.95*30)-1],"p99_ms":values[-1],"max_ms":values[-1]})
    dest=ROOT/"docs/validation/M4/budgets";dest.mkdir(parents=True,exist_ok=True)
    for path in [measured,inputs]: (dest/path.name).write_bytes(path.read_bytes())
    summary={"schema":"rust-mcp-m4-budgets-v1","status":"passed","synchronous_ceiling_ms":60000,
             "image_id":receipt["image_id"],"binary_sha256":receipt["binary_sha256"],
             "qualification_scope":"Measured operation path via Tasks on a frozen binary; synchronous protocol routing and final source-bound gate require separate receipts",
             "calibration_excluded":receipt["calibration_excluded_from_operation_timer"],
             "cold_definition":receipt["cold_definition"],"groups":groups,
             "outputs":[{"path":str((dest/p.name).relative_to(ROOT)),"sha256":hashlib.sha256(p.read_bytes()).hexdigest()} for p in [measured,inputs]]}
    (ROOT/"docs/validation/M4/budgets.json").write_text(json.dumps(summary,indent=2)+"\n")
    print("PASS 300 M4 observations; max ms",max(r["elapsed_ms"] for r in rows))
if __name__=="__main__":
    if not __debug__: raise RuntimeError("Optimized Python mode is rejected")
    main()
