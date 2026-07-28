"""Emit `name=value,...` for every centipawn margin scaled by a factor.

The margins are the parameters that get compared directly against an
evaluation score. Depths, move counts, divisors and reductions are not on
that scale and must not move with it -- scaling a depth limit by 1.35 is
not a wider margin, it is a different search.
"""
import re, sys
f = float(sys.argv[1])
s = open('/root/kestrel_joao/Kestrel/src/search.rs').read()
names = re.findall(r'"([^"]+)"', re.search(r'pub const PARAM_NAMES[^=]*= \[\n(.*?)\n\];', s, re.S).group(1))
i = s.index("impl Default for SearchParams {")
d = s[i:s.index("\n}", s.index("Self {", i))]
vals = {}
for m in re.finditer(r'(\w+): (?:DepthMargin \{ base: (-?\d+), slope: (-?\d+) \}|(-?\d+))', d):
    if m.group(4) is not None:
        vals[m.group(1)] = int(m.group(4))
    else:
        vals[m.group(1) + '_base'] = int(m.group(2))
        vals[m.group(1) + '_slope'] = int(m.group(3))
NOT_CP = ('limit', 'divisor', 'mult', 'power', 'offset', 'factor', 'count', 'max_',
          '_reduction', 'reduction_scale', 'min_depth', 'pruning_max_depth',
          'widening', 'min_asp', '_quad', '_linear', 'bonus_max', 'malus_max')
EXTRA = {'razor_per_depth', 'nmp_static_eval_depth_margin', 'do_deeper_margin_depth'}
cp = [n for n in names if n in vals and (n in EXTRA or not any(k in n for k in NOT_CP)) and vals[n] != 0]
print(",".join(f"{n}={round(vals[n] * f)}" for n in cp))
