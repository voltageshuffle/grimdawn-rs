# IDA script (headless): writes the symbol table's own names into the open database, so the decompile reads
# like the comments. Export rows are skipped: the binary's names are already right.
#   MODULE=Game.dll TABLE=<re>/<version>/symbols.tsv idat -A -Sscripts/re-apply-names.py <re>/<version>/idb/Game.dll.i64
# It saves on exit (idat -A). Do not call save_database here as well: saving twice corrupts the .i64.
import os

import ida_name
import idaapi

idaapi.auto_wait()
module = os.environ["MODULE"]
ok = bad = 0
for line in open(os.environ["TABLE"]):
    if line.startswith("#") or not line.strip():
        continue
    name, kind, mod, value, origin = (line.rstrip("\n").split("\t") + [""] * 5)[:5]
    if mod != module or kind not in ("fn", "global") or origin == "export":
        continue
    if not value.startswith("0x"):
        continue
    flat = name.replace("::", "__").replace("~", "dtor_")
    done = ida_name.set_name(
        int(value, 16), flat, ida_name.SN_NOWARN | ida_name.SN_NOCHECK | ida_name.SN_FORCE
    )
    ok += bool(done)
    bad += not done
print(f"applied {ok}, failed {bad}")
idaapi.qexit(0)
