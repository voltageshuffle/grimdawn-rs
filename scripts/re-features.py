# IDA script (headless): one JSON line per function of the open database, the evidence `re-match` pairs
# functions on across game versions. Run it on a copy of the .i64, never on one another agent has open:
#   DUMP_OUT=<version>/<module>.jsonl idat -A -Sscripts/re-features.py <copy>.i64
# Fields: ea, name, size, mh (hash of the mnemonic sequence), n (instruction count), calls (callee names,
# "?" for unnamed), ct (callee addresses), strs (string literals referenced).
import hashlib
import json
import os

import ida_auto
import ida_funcs
import idaapi
import idc
import idautils

ida_auto.auto_wait()
with open(os.environ["DUMP_OUT"], "w") as out:
    for ea in idautils.Functions():
        fn = ida_funcs.get_func(ea)
        mn, calls, cts, strs = [], [], [], []
        for h in idautils.FuncItems(ea):
            m = idc.print_insn_mnem(h)
            mn.append(m)
            if m == "call":
                t = idc.get_operand_value(h, 0)
                n = idc.get_func_name(t) or idc.get_name(t) or ""
                calls.append(n if n and not n.startswith("sub_") else "?")
                cts.append(t)
            for x in idautils.DataRefsFrom(h):
                ty = idc.get_str_type(x)
                s = idc.get_strlit_contents(x, -1, ty) if ty is not None else None
                if s:
                    strs.append(s.decode("latin-1")[:80])
        out.write(
            json.dumps(
                {
                    "ea": ea,
                    "name": idc.get_func_name(ea),
                    "size": fn.end_ea - fn.start_ea,
                    "mh": hashlib.md5(" ".join(mn).encode()).hexdigest()[:16],
                    "n": len(mn),
                    "calls": calls,
                    "ct": cts,
                    "strs": strs,
                }
            )
            + "\n"
        )
idaapi.qexit(0)
