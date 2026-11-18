"""Model bounds of every unit (world units, gun barrels and muzzles left out) next to its JSON size and
hit_box: the source of the hit_box numbers. Usage: python3 tools/model_bounds.py [assets] [out.json]"""
import json, struct, sys, glob, os
import numpy as np
root = sys.argv[1] if len(sys.argv) > 1 else 'assets'
EXCL = ['barrel','muzzle']
def quat(q):
    x,y,z,w = q
    return np.array([[1-2*(y*y+z*z),2*(x*y-z*w),2*(x*z+y*w)],[2*(x*y+z*w),1-2*(x*x+z*z),2*(y*z-x*w)],[2*(x*z-y*w),2*(y*z+x*w),1-2*(x*x+y*y)]])
def local(n):
    if 'matrix' in n: return np.array(n['matrix']).reshape(4,4).T
    m = np.eye(4)
    r = quat(n.get('rotation',[0,0,0,1])); s = np.array(n.get('scale',[1,1,1]))
    m[:3,:3] = r*s; m[:3,3] = n.get('translation',[0,0,0]); return m
def glb_bounds(path):
    data = open(path,'rb').read()
    l = struct.unpack('<I', data[12:16])[0]
    g = json.loads(data[20:20+l])
    lo = np.full(3, 1e9); hi = -lo
    def walk(i, pm):
        n = g['nodes'][i]; m = pm @ local(n)
        if 'mesh' in n and not any(s in n.get('name','').lower() for s in EXCL):
            for p in g['meshes'][n['mesh']]['primitives']:
                a = g['accessors'][p['attributes']['POSITION']]
                mn, mx = np.array(a['min']), np.array(a['max'])
                for c in range(8):
                    v = np.array([mx[k] if c>>k&1 else mn[k] for k in range(3)]+[1.0])
                    w = (m@v)[:3]; lo[:] = np.minimum(lo,w); hi[:] = np.maximum(hi,w)
        for ch in n.get('children',[]): walk(ch, m)
    sc = g['scenes'][g.get('scene',0)]
    for r in sc['nodes']: walk(r, np.eye(4))
    return lo, hi
out = {}
for f in sorted(glob.glob(os.path.join(root,'data/factions/*.json'))):
    for u in json.load(open(f,encoding='utf-8'))['units']:
        md = u.get('model',{}); s = md.get('scale',1.0)
        lo = np.full(3,1e9); hi = -lo
        if md.get('mesh'):
            a,b = glb_bounds(os.path.join(root, md['mesh'])); lo=np.minimum(lo,a); hi=np.maximum(hi,b)
        for bx in md.get('boxes',[]):
            p = np.array(bx['pos']); h = np.array(bx['size'])/2
            lo=np.minimum(lo,p-h); hi=np.maximum(hi,p+h)
        if lo[0] > 1e8: continue
        lo*=s; hi*=s
        out[u['id']] = dict(file=os.path.basename(f), lo=lo.round(3).tolist(), hi=hi.round(3).tolist(), size=u['size'], hb=u.get('hit_box'), fp=u.get('footprint',0), cats=u.get('categories',[]), kind=u.get('move',{}).get('kind'))
if len(sys.argv) > 2:
    json.dump(out, open(sys.argv[2],'w'), indent=0)
for k,v in out.items():
    lo,hi=v['lo'],v['hi']
    cx=(lo[0]+hi[0])/2; cz=(lo[2]+hi[2])/2
    print(f"{k:28s} c{cx:5.2f},{cz:5.2f} {v['file'][:6]} model w{hi[0]-lo[0]:6.2f} h{hi[1]-lo[1]:6.2f} (y {lo[1]:5.2f}..{hi[1]:5.2f}) l{hi[2]-lo[2]:6.2f} | size {v['size']} hit_box {v['hb']} fp{v['fp']}")
