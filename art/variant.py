"""Variants of the mod's own models (art/models_variants.py): the mod's GLB is imported unchanged (its parts,
names, animations, UVs and texture stay), new parts are added with the kit, and the texture is extended: the
mod's 512x512 albedo / team mask / normal map become the top half of a 512x1024 set whose bottom half holds the
new parts' baked tiles. Exported with front +y, so the mod's node transforms are kept as they are."""
import math
import os

import bmesh
import bpy
import numpy as np
from mathutils import Matrix, Vector
from PIL import Image

import kit
from kit import Kit, Part, TEX

MODELS = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets", "models")


def _load(path, n):
    im = Image.open(path).convert("RGB").resize((n, n))
    return np.asarray(im, np.float32) / 255.0


class Variant(Kit):
    def __init__(self, name, src, scale, brightness=2.1):
        super().__init__(name)
        self.parts = []
        self.front = "+y"
        self.src, self.scale, self.brightness = src, scale, brightness
        before = set(bpy.data.objects)
        bpy.ops.import_scene.gltf(filepath=os.path.join(MODELS, src + ".glb"))
        self.imported = [o for o in bpy.data.objects if o not in before]
        self.ext = {o.name: o for o in self.imported}
        # additions are written in world units, game front (-y): turn and shrink them into the mod's model space
        self.xf = [Matrix.Rotation(math.pi, 4, "Z") @ Matrix.Scale(1 / scale, 4)]
        self.root = None

    def world_of(self, node):
        """Where an imported node sits, in world units with front -y (for placing additions)."""
        m = Matrix.Rotation(math.pi, 4, "Z") @ Matrix.Scale(self.scale, 4) @ self.ext[node].matrix_world
        return m.to_translation()

    def on(self, node, name=None):
        """A new part that moves with the imported node `node` (a turret, a torso...)."""
        p = Part(self, name or f"{node}_add{len(self.parts)}", (0, 0, 0), None)
        p.ext_parent = self.ext[node]
        self.parts.append(p)
        self.cur = p
        return p

    def finish(self, smart_angle=40.0):
        scene = bpy.context.scene
        bpy.context.view_layer.update()
        # hit box over the mod's meshes and the additions, in world units
        lo, hi = Vector((1e9,) * 3), Vector((-1e9,) * 3)
        objs = [o for o in self.imported if o.type == "MESH"] + [ob for p in self.parts for ob in p.pieces]
        for o in objs:
            nm = o.name.lower()
            if any(w in nm for w in ("barrel", "muzzle")) or o.name in self.nohit_pieces:
                continue
            if o in self.imported and o.parent and any(w in o.parent.name.lower() for w in ("barrel", "muzzle")):
                continue
            for v in o.data.vertices:
                w = o.matrix_world @ v.co
                lo, hi = Vector(map(min, lo, w)), Vector(map(max, hi, w))
        s = self.scale
        self.hit_box = [round(max(hi.x, -lo.x) * 2 * s, 2), round(hi.z * s, 2), round(max(hi.y, -lo.y) * 2 * s, 2)]
        # join the additions per part, parent them to their node
        new = []
        for p in self.parts:
            bpy.ops.object.select_all(action="DESELECT")
            for o in p.pieces:
                o.select_set(True)
            bpy.context.view_layer.objects.active = p.pieces[0]
            if len(p.pieces) > 1:
                bpy.ops.object.join()
            ob = bpy.context.view_layer.objects.active
            ob.name = ob.data.name = p.name
            ob.parent = p.ext_parent
            ob.matrix_parent_inverse = p.ext_parent.matrix_world.inverted()
            p.obj = ob
            new.append(ob)
        bpy.context.view_layer.update()
        # UVs of the additions into the bottom part of the atlas, the mod's into the top half
        bpy.ops.object.select_all(action="DESELECT")
        for o in new:
            o.select_set(True)
        bpy.context.view_layer.objects.active = new[0]
        bpy.ops.object.mode_set(mode="EDIT")
        bpy.ops.mesh.select_all(action="SELECT")
        bpy.ops.uv.smart_project(angle_limit=math.radians(smart_angle), island_margin=0.0, area_weight=0.0, scale_to_bounds=False)
        bpy.ops.uv.pack_islands(margin=4.0 / TEX, rotate=True, shape_method="AABB", margin_method="FRACTION")
        bpy.ops.object.mode_set(mode="OBJECT")
        for o in new:
            for d in o.data.uv_layers.active.data:
                d.uv = (d.uv[0], d.uv[1] * 0.48)
        for o in self.imported:
            if o.type == "MESH" and o.data.uv_layers:
                for d in o.data.uv_layers[0].data:
                    d.uv = (d.uv[0], 0.5 + 0.5 * d.uv[1])
        W, H = TEX, TEX * 2
        bake = {}
        scene.render.engine = "CYCLES"
        scene.cycles.device = "CPU"
        scene.cycles.samples = 4
        scene.render.bake.use_pass_direct = False
        scene.render.bake.use_pass_indirect = False
        scene.render.bake.use_pass_color = True
        mats = {m for o in new for m in o.data.materials}
        for role, src in (("albedo", "tile_color"), ("team", "tile_mask")):
            img = bpy.data.images.new("_bake_" + role, W, H, alpha=False)
            for m in mats:
                nt = m.node_tree
                bsdf = next(n for n in nt.nodes if n.type == "BSDF_PRINCIPLED")
                nt.links.new(nt.nodes[src].outputs["Color"], bsdf.inputs["Base Color"])
                tgt = nt.nodes.get("bake_target") or nt.nodes.new("ShaderNodeTexImage")
                tgt.name = "bake_target"
                tgt.image = img
                nt.nodes.active = tgt
            bpy.ops.object.select_all(action="DESELECT")
            for o in new:
                o.select_set(True)
            bpy.ops.object.bake(type="DIFFUSE", pass_filter={"COLOR"}, use_clear=True, margin=6)
            px = np.array(img.pixels[:], np.float32).reshape(H, W, 4)[::-1, :, :3]  # rows from the top
            bake[role] = px
        # compose: mod texture on top, additions below (albedo divided by the model's brightness)
        out = {}
        for role, suffix in (("albedo", "albedo"), ("team", "team"), ("normal", "normal")):
            top = _load(os.path.join(MODELS, f"{self.src}_{suffix}.png"), W) if os.path.exists(os.path.join(MODELS, f"{self.src}_{suffix}.png")) else None
            full = np.zeros((H, W, 3), np.float32)
            if role == "albedo":
                full[W:] = np.clip(bake["albedo"][W:] / self.brightness, 0, 1)
            elif role == "team":
                full[W:] = bake["team"][W:]
            else:
                full[:] = (0.5, 0.5, 1.0)
            if top is not None:
                full[:W] = top
            elif role == "normal":
                continue
            img = bpy.data.images.new(f"{self.name}_{suffix}", W, H, alpha=False)
            if role != "albedo":
                img.colorspace_settings.name = "Non-Color"
            px = np.ones((H, W, 4), np.float32)
            px[..., :3] = full[::-1]
            img.pixels.foreach_set(px.ravel())
            out[role] = img
        unit = bpy.data.materials.new("unit")
        unit.use_nodes = True
        nt = unit.node_tree
        bsdf = next(n for n in nt.nodes if n.type == "BSDF_PRINCIPLED")
        a = nt.nodes.new("ShaderNodeTexImage")
        a.image = out["albedo"]
        nt.links.new(a.outputs["Color"], bsdf.inputs["Base Color"])
        t = nt.nodes.new("ShaderNodeTexImage")
        t.name = t.label = "team"
        t.image = out["team"]
        if "normal" in out:
            ni = nt.nodes.new("ShaderNodeTexImage")
            ni.image = out["normal"]
            nm = nt.nodes.new("ShaderNodeNormalMap")
            nt.links.new(ni.outputs["Color"], nm.inputs["Color"])
            nt.links.new(nm.outputs["Normal"], bsdf.inputs["Normal"])
        for o in [o for o in self.imported if o.type == "MESH"] + new:
            o.data.materials.clear()
            o.data.materials.append(unit)
            for poly in o.data.polygons:
                poly.material_index = 0
        self._actions()
        return out
