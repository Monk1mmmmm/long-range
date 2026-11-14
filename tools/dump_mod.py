import glob, os, sys, json
sys.path.insert(0, os.path.dirname(__file__))
import bp
M = sys.argv[1]
for f in sorted(glob.glob(M + "/units/*/*_unit.bp") + glob.glob(M + "/hook/units/*/*_unit.bp")):
    b = bp.load(f)
    uid = os.path.basename(f).split("_")[0]
    g, ph, d, e, i, air = b.get("General", {}), b.get("Physics", {}), b.get("Display", {}), b.get("Economy", {}), b.get("Intel", {}), b.get("Air", {})
    print(f"== {uid} '{g.get('UnitName','')}' / {b.get('Description','')} tech={g.get('TechLevel','')} scale={d.get('UniformScale')} size=({b.get('SizeX')},{b.get('SizeY')},{b.get('SizeZ')}) foot={b.get('Footprint')} skirt=({ph.get('SkirtSizeX')},{ph.get('SkirtSizeZ')}) hp={b.get('Defense',{}).get('MaxHealth')} armor={b.get('Defense',{}).get('ArmorType')}")
    print(f"   move: {ph.get('MotionType')} speed={ph.get('MaxSpeed')} rev={ph.get('MaxSpeedReverse')} acc={ph.get('MaxAcceleration')} brake={ph.get('MaxBrake')} turn={ph.get('TurnRate')} | air: speed={air.get('MaxAirspeed')} turn={air.get('TurnSpeed')} | elev={ph.get('Elevation')}")
    print(f"   eco: M{e.get('BuildCostMass')} E{e.get('BuildCostEnergy')} T{e.get('BuildTime')} rate={e.get('BuildRate')} maxdist={e.get('MaxBuildDistance')} | intel: vis={i.get('VisionRadius')} radar={i.get('RadarRadius')}")
    for w in b.get("Weapon", []) or []:
        if not isinstance(w, dict) or w.get("WeaponCategory") == "Death": continue
        print(f"   W '{w.get('DisplayName')}' dmg={w.get('Damage')} r={w.get('DamageRadius')} type={w.get('DamageType')} range={w.get('MinRadius')}-{w.get('MaxRadius')} rof={w.get('RateOfFire')} mv={w.get('MuzzleVelocity')} salvo={w.get('MuzzleSalvoSize')} tol={w.get('FiringTolerance')} rand={w.get('FiringRandomness')} yawspd={w.get('TurretYawSpeed')} pitchspd={w.get('TurretPitchSpeed')} bones=({w.get('TurretBoneYaw')},{w.get('TurretBonePitch')},{w.get('TurretBoneMuzzle')}) layer={w.get('FireTargetLayerCapsTable')} ballistic={w.get('BallisticArc')} proj={w.get('ProjectileId')} label={w.get('Label')}")
