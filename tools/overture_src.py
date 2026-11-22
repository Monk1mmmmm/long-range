"""Overture Maps as a stand-in for the Overpass API (tools/osm_map.py --source overture).

Overture Maps (overturemaps.org; built from OpenStreetMap and other open data, CDLA-Permissive / ODbL) is published
as Parquet files on a public S3 bucket. This reads the buildings, transportation, base (land, land use, land cover,
water, infrastructure) themes for a rectangle and returns the same element dictionaries Overpass would
(`{"type": "way", "tags": {...}, "geometry": [{"lat", "lon"}, ..]}` plus `_part`), so the rest of osm_map.py
does not know the difference. Needs `pip install pyarrow shapely` (besides numpy and Pillow); no extra hosts but
the S3 bucket. Results are cached as JSON in tools/out/osm_cache.

The data is the present day. A map of the past is made by dropping what did not exist yet: `drop_names` (regex on
the road's name) and `drop_bridges` (a rectangle: road bridges inside it, with the viaduct ramps) - see osm_map.py.
"""
import json
import os
import re
import sys

RELEASE = os.environ.get("LR_OVERTURE_RELEASE", "2026-11-18.0")
BUCKET = "overturemaps-us-west-2/release"

# Overture road classes -> highway=; walking-only classes are left out, except `pedestrian` (the streets of an old
# town, which units can drive along), which becomes a living street.
ROAD = {"motorway": "motorway", "trunk": "trunk", "primary": "primary", "secondary": "secondary", "tertiary": "tertiary",
        "unclassified": "unclassified", "residential": "residential", "living_street": "living_street",
        "service": "service", "track": "track", "pedestrian": "living_street"}
UNPAVED = {"unpaved", "dirt", "gravel", "ground", "grass", "sand", "compacted", "fine_gravel", "mud", "earth"}
BUILDING = {"religious": "church", "education": "school", "civic": "civic", "medical": "hospital", "commercial": "commercial",
            "industrial": "industrial", "agricultural": "farm", "military": "bunker", "outbuilding": "shed",
            "transportation": "transportation", "service": "service", "entertainment": "commercial"}
LAND_USE = {"residential": {"landuse": "residential"}, "industrial": {"landuse": "industrial"},
            "commercial": {"landuse": "commercial"}, "retail": {"landuse": "retail"}, "cemetery": {"landuse": "cemetery"},
            "park": {"leisure": "park"}, "garden": {"leisure": "park"}, "grass": {"landuse": "grass"},
            "meadow": {"landuse": "meadow"}, "farmland": {"landuse": "farmland"}, "orchard": {"landuse": "orchard"},
            "vineyard": {"landuse": "vineyard"}, "allotments": {"landuse": "allotments"},
            "construction": {"landuse": "construction"}, "landfill": {"landuse": "brownfield"},
            "quarry": {"landuse": "quarry"}, "farmyard": {"landuse": "farmyard"}, "village_green": {"landuse": "grass"},
            "brownfield": {"landuse": "brownfield"}, "pedestrian": {"landuse": "retail"}}
LAND = {"wood": {"landuse": "forest"}, "forest": {"landuse": "forest"}, "scrub": {"natural": "scrub"},
        "heath": {"natural": "heath"}, "bare_rock": {"natural": "bare_rock"}, "scree": {"natural": "scree"},
        "rock": {"natural": "bare_rock"}, "beach": {"natural": "beach"}, "sand": {"natural": "sand"},
        "grass": {"natural": "grassland"}, "grassland": {"natural": "grassland"}}
# ESA WorldCover (Overture land_cover, 10 m): only where it adds what the other layers lack.
COVER = {"forest": {"landuse": "forest"}, "shrub": {"natural": "scrub"}, "barren": {"natural": "bare_rock"},
         "grass": {"natural": "grassland"}}


def _dataset(theme, typ):
    import pyarrow.dataset as ds
    import pyarrow.fs as fs
    s3 = fs.S3FileSystem(anonymous=True, region="us-west-2")
    return ds.dataset(f"{BUCKET}/{RELEASE}/theme={theme}/type={typ}/", filesystem=s3, format="parquet")


def _rows(theme, typ, want, box):
    import pyarrow.dataset as ds
    d = _dataset(theme, typ)
    cols = [c for c in want if c in d.schema.names]
    s, w, n, e = box
    flt = (ds.field("bbox", "xmax") > w) & (ds.field("bbox", "xmin") < e) & (ds.field("bbox", "ymax") > s) & (ds.field("bbox", "ymin") < n)
    return d.to_table(filter=flt, columns=cols).to_pylist()


def _polys(g, clip):
    """Polygons (exterior, holes) of a shapely geometry, clipped to the rectangle."""
    from shapely.validation import make_valid
    if not g.is_valid:
        g = make_valid(g)
    g = g.intersection(clip)
    out = []
    parts = list(g.geoms) if hasattr(g, "geoms") else [g]
    for p in parts:
        if p.geom_type == "Polygon" and not p.is_empty:
            out.append(([c for c in p.exterior.coords], [[c for c in r.coords] for r in p.interiors]))
        elif p.geom_type in ("MultiPolygon", "GeometryCollection"):
            out += _polys(p, clip)
    return out


def _ll(coords):
    return [{"lat": y, "lon": x} for x, y in coords]


def _area(tags, part, outer, holes):
    """An area as a way, or as a multipolygon relation when it has holes."""
    if not holes:
        return {"type": "way", "tags": tags, "geometry": _ll(outer), "_part": part}
    members = [{"role": "outer", "geometry": _ll(outer)}] + [{"role": "inner", "geometry": _ll(h)} for h in holes]
    return {"type": "relation", "tags": tags, "members": members, "_part": part}


def fetch(geo, cache_dir, name):
    """All elements for the square of `geo` (cached). Roads carry `_id`, `_conn` (connector ids) and `_name`."""
    from shapely import wkb
    from shapely.geometry import box as sbox
    pad = 0.004
    b = (geo.s - pad, geo.w - pad, geo.n + pad, geo.e + pad)  # S, W, N, E
    path = os.path.join(cache_dir, f"overture_{name}_{RELEASE}_{b[0]:.4f}_{b[1]:.4f}_{b[2]:.4f}_{b[3]:.4f}.json")
    if os.path.exists(path):
        with open(path, encoding="utf-8") as f:
            return json.load(f)
    clip = sbox(b[1], b[0], b[3], b[2])
    els = []

    def say(what, k):
        print(f"  overture {what}: {k}", file=sys.stderr)

    # buildings
    rows = _rows("buildings", "building", ["geometry", "subtype", "class"], b)
    for r in rows:
        cls, sub = r.get("class"), r.get("subtype")
        tag = BUILDING.get(sub, "yes") if not cls or cls in ("residential", "service") else cls
        if sub == "religious":
            tag = "church" if cls in (None, "church", "cathedral") else cls
        for outer, _ in _polys(wkb.loads(r["geometry"]), clip):
            els.append({"type": "way", "tags": {"building": tag}, "geometry": _ll(outer), "_part": "buildings"})
    say("buildings", len(rows))

    # land use / land / land cover / water
    for r in _rows("base", "land_use", ["geometry", "class"], b):
        tags = LAND_USE.get(r.get("class"))
        if tags:
            for outer, holes in _polys(wkb.loads(r["geometry"]), clip):
                els.append(_area(dict(tags), "land", outer, holes))
    for r in _rows("base", "land", ["geometry", "subtype", "class"], b):
        g = wkb.loads(r["geometry"])
        cls, sub = r.get("class"), r.get("subtype")
        if g.geom_type == "Point":
            nat = {"tree": "tree", "peak": "peak", "rock": "rock", "stone": "stone"}.get(cls) or {"tree": "tree", "peak": "peak"}.get(sub)
            if nat:
                els.append({"type": "node", "tags": {"natural": nat}, "lat": g.y, "lon": g.x, "_part": "land"})
            continue
        tags = LAND.get(cls) or LAND.get(sub)
        if tags:
            for outer, holes in _polys(g, clip):
                els.append(_area(dict(tags), "land", outer, holes))
    for r in _rows("base", "land_cover", ["geometry", "subtype"], b):
        tags = COVER.get(r.get("subtype"))
        if tags:
            for outer, holes in _polys(wkb.loads(r["geometry"]), clip):
                els.append(_area(dict(tags), "cover", outer, holes))
    for r in _rows("base", "water", ["geometry", "subtype", "class"], b):
        g = wkb.loads(r["geometry"])
        sub, cls = r.get("subtype"), r.get("class")
        if g.geom_type not in ("Polygon", "MultiPolygon"):
            continue
        if sub == "ocean":
            for outer, holes in _polys(g, clip):
                els.append(_area({"_sea": "yes"}, "sea", outer, holes))
        elif sub in ("lake", "pond", "reservoir", "water") and cls not in ("swimming_pool", "fountain", "moat", "tidal_channel"):
            for outer, holes in _polys(g, clip):
                els.append(_area({"natural": "water"}, "land", outer, holes))
        elif sub == "river":
            for outer, holes in _polys(g, clip):
                els.append(_area({"waterway": "riverbank"}, "land", outer, holes))
    for r in _rows("base", "infrastructure", ["geometry", "subtype", "class"], b):
        if r.get("class") == "power_tower":
            g = wkb.loads(r["geometry"])
            if g.geom_type == "Point":
                els.append({"type": "node", "tags": {"power": "tower"}, "lat": g.y, "lon": g.x, "_part": "land"})

    # roads
    rows = _rows("transportation", "segment", ["id", "geometry", "subtype", "class", "names", "road_flags", "road_surface", "connectors"], b)
    for r in rows:
        if r.get("subtype") != "road" or r.get("class") not in ROAD:
            continue
        g = wkb.loads(r["geometry"])
        if g.geom_type != "LineString":
            continue
        flags = {v for x in (r.get("road_flags") or []) for v in (x.get("values") or [])}
        surf = {x.get("value") for x in (r.get("road_surface") or []) if x.get("value")}
        tags = {"highway": ROAD[r["class"]]}
        if "is_bridge" in flags:
            tags["bridge"] = "yes"
        if "is_tunnel" in flags:
            tags["tunnel"] = "yes"
        if surf & UNPAVED:
            tags["surface"] = sorted(surf & UNPAVED)[0]
        nm = (r.get("names") or {}).get("primary") or ""
        els.append({"type": "way", "tags": tags, "geometry": _ll(g.coords), "_part": "roads", "_id": r["id"], "_class": r["class"],
                    "_name": nm, "_conn": [c.get("connector_id") for c in (r.get("connectors") or [])]})
    say("road segments", len(rows))
    os.makedirs(cache_dir, exist_ok=True)
    with open(path, "w", encoding="utf-8") as f:
        json.dump(els, f)
    return els


def drop_future(els, drop_names=None, drop_bridges=None, report=print):
    """Takes out what a map of the past must not have.

    drop_names: regex on a road's name (matched case-insensitively), the road is dropped.
    drop_bridges: (S, W, N, E): bridges of primary roads and better inside it are dropped, and then the
    primary-road stubs left dangling at either end of them (the road that led to the bridge).
    """
    gone = set()
    for el in els:
        if el.get("_part") != "roads":
            continue
        nm = el.get("_name", "")
        if drop_names and nm and re.search(drop_names, nm, re.I):
            gone.add(el["_id"])
        if drop_bridges and el["tags"].get("bridge") and el["_class"] in ("motorway", "trunk", "primary"):
            s, w, n, e = drop_bridges
            pts = el["geometry"]
            mid = pts[len(pts) // 2]
            if s <= mid["lat"] <= n and w <= mid["lon"] <= e:
                gone.add(el["_id"])
    if not gone:
        return els
    roads = [el for el in els if el.get("_part") == "roads"]
    # Dangling approaches: a primary/trunk road inside the box that now ends at a connector no one else uses.
    if drop_bridges:
        s, w, n, e = drop_bridges
        for _ in range(8):
            use = {}
            for el in roads:
                if el["_id"] not in gone:
                    for c in el["_conn"]:
                        use[c] = use.get(c, 0) + 1
            dropped = False
            for el in roads:
                if el["_id"] in gone or el["_class"] not in ("trunk", "primary", "motorway"):
                    continue
                pts = el["geometry"]
                mid = pts[len(pts) // 2]
                if not (s <= mid["lat"] <= n and w <= mid["lon"] <= e):
                    continue
                ends = [el["_conn"][0], el["_conn"][-1]] if el["_conn"] else []
                if any(use.get(c, 0) <= 1 for c in ends) and any(any(c in g_["_conn"] for c in ends) for g_ in roads if g_["_id"] in gone):
                    gone.add(el["_id"])
                    dropped = True
            if not dropped:
                break
    named = sorted({el["_name"] for el in roads if el["_id"] in gone and el["_name"]})
    report(f"dropped {len(gone)} road segments of the future" + (f" ({', '.join(named)})" if named else ""))
    return [el for el in els if el.get("_id") not in gone]
