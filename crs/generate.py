"""Write <out>/EPSG:<code>.json for EPSG:4326 and every non-deprecated EPSG projected CRS:
{ "projjson": ..., "tileMatrixSet": { "origin": [minX, maxY], "extentAtZoom0": ... } },
the grid being the square from the top-left corner of the CRS's area of use, covering it.

    pip install pyproj && python generate.py <out>
"""

import json
import math
import sys
from pathlib import Path

from pyproj import CRS, Transformer
from pyproj.database import query_crs_info
from pyproj.enums import PJType

out = Path(sys.argv[1])
out.mkdir(parents=True, exist_ok=True)
wgs84 = CRS.from_epsg(4326)

infos = query_crs_info(auth_name="EPSG", pj_types=PJType.PROJECTED_CRS)
codes = ["4326"] + [i.code for i in infos if i.area_of_use is not None]
written = 0
for code in codes:
    try:
        crs = CRS.from_epsg(int(code))
        a = crs.area_of_use
        # easting first, whatever the CRS's axis order
        bounds = Transformer.from_crs(wgs84, crs, always_xy=True).transform_bounds(
            a.west, a.south, a.east, a.north, densify_pts=21
        )
    except Exception as e:  # noqa: BLE001 - a CRS PROJ cannot handle is skipped
        print(f"skip EPSG:{code}: {e}", file=sys.stderr)
        continue
    min_x, min_y, max_x, max_y = bounds
    if not all(map(math.isfinite, bounds)) or min_x >= max_x or min_y >= max_y:
        print(f"skip EPSG:{code}: bounds {bounds}", file=sys.stderr)
        continue
    entry = {
        "projjson": crs.to_json_dict(),
        "tileMatrixSet": {
            "origin": [min_x, max_y],
            "extentAtZoom0": max(max_x - min_x, max_y - min_y),
        },
    }
    (out / f"EPSG:{code}.json").write_text(json.dumps(entry, separators=(",", ":")))
    written += 1
print(f"{written} of {len(codes)} CRSs written", file=sys.stderr)
