"""Auto-rig a Tripo stalker and bake the Crawl/Idle/Jump/Run/Walk clips into a GLB.

The unrigged sources below (soldier/uhd/soldier.glb, soldier/medium, stalker/*) are
not in assets/ any more; restore them from the asset archive first (CLAUDE.md, Asset
tiers).

Medium runtime build (raw Tripo export, grounded, front faces -Y):
  blender -b --factory-startup --python-exit-code 1 --python tools/rig_stalker.py -- \
      raw.glb rigged.glb
  npx @gltf-transform/cli@4 resize rigged.glb assets/models/stalker/medium/stalker.glb \
      --width 2048 --height 2048

Soldier build, the player model (assets/models/soldier/uhd/soldier.glb: glTF +Z front,
arms hanging against the body, unrigged, normal mapped, ~2M triangles). Bone heat runs
on the medium scan of the same model and its weights are copied onto the full mesh:
  blender -b --factory-startup --python-exit-code 1 --python tools/rig_stalker.py -- \
      assets/models/soldier/uhd/soldier.glb assets/models/soldier/uhd/soldier_rigged.glb \
      --height 0.977325 --weights-from assets/models/soldier/medium/soldier.glb \
      --image-format AUTO
  python3 tools/compress_glb_textures.py assets/models/soldier/uhd/soldier_rigged.glb

Cordon guards (soldiers/medium/soldier-{f1,f2,m2}.glb from the Tripo workspace: glTF +X
front, T-pose, ~5k triangles). A wider Walk arm swing keeps their hands moving when seen from
the tactical camera:
  blender -b --factory-startup --python-exit-code 1 --python tools/rig_stalker.py -- \
      soldier-f1.glb assets/models/soldiers/medium/soldier-f1.glb \
      --yaw-degrees -90 --height 0.977325 --t-pose --walk-arms 32 --image-format AUTO
  python3 tools/compress_glb_textures.py assets/models/soldiers/medium/soldier-f1.glb
soldier-m1 was rigged by Tripo (`animate_rig`, biped; the Tripo workspace's
tripo-out/soldier-m1-rig-rig-cd280954/model.glb): the same build with --armature keeps
Tripo's skeleton and skin weights and bakes the clips onto them:
  blender -b --factory-startup --python-exit-code 1 --python tools/rig_stalker.py -- \
      model.glb assets/models/soldiers/medium/soldier-m1.glb --armature \
      --yaw-degrees -90 --height 0.977325 --t-pose --walk-arms 32 --image-format AUTO

New Harmont townsfolk (the Tripo workspace's townsfolk/: uhd/ the raw Tripo scans, glTF +Z
front, 4096 px textures kept native). The two ~2M-triangle scans are simplified at a sub-
millimetre error that renders identical to the source up close: ~100k triangles in medium/,
and ~13k in low/ for bone heat, whose weights are copied onto the medium mesh:
  W=~/Projects/agentic/RoadSide-Picnic/models/townsfolk
  npx @gltf-transform/cli@4 simplify $W/uhd/scientist.glb $W/medium/scientist.glb --ratio 0 --error 0.0002
  npx @gltf-transform/cli@4 simplify $W/uhd/scientist.glb $W/low/scientist.glb --ratio 0 --error 0.001
  (the same for factory-worker)
The factory worker hangs his arms like the soldier:
  blender -b --factory-startup --python-exit-code 1 --python tools/rig_stalker.py -- \
      $W/medium/factory-worker.glb assets/models/townsfolk/medium/factory-worker.glb \
      --height 0.977325 --weights-from $W/low/factory-worker.glb --image-format AUTO
The scientist gestures with both arms bent: his arm joints are given (tools/rig_joints/),
the head centered on the spine, and the clips hang the arms:
  blender -b --factory-startup --python-exit-code 1 --python tools/rig_stalker.py -- \
      $W/medium/scientist.glb assets/models/townsfolk/medium/scientist.glb \
      --center --joints tools/rig_joints/scientist.json --hang-arms \
      --height 0.977325 --weights-from $W/low/scientist.glb --image-format AUTO
The construction worker is Tripo-rigged (humanoid bone names, 4.8k triangles as delivered):
  blender -b --factory-startup --python-exit-code 1 --python tools/rig_stalker.py -- \
      $W/uhd/construction-worker.glb assets/models/townsfolk/medium/construction-worker.glb \
      --armature --yaw-degrees -90 --height 0.977325 --t-pose --image-format AUTO
Then, last, for all three (the bake step only repairs the few zero tangents Blender writes):
  python3 tools/compress_glb_textures.py assets/models/townsfolk/medium/*.glb
  glb_bake_tangents.py assets/models/townsfolk/medium/*.glb   # agentic-tooling media/

UHD build (assets/models/stalker/uhd/stalker.glb: glTF +X front, arms outstretched,
unrigged, 8192 px JPEG). Keep the full geometry and texture; the output is the source
mesh skinned as-is:
  blender -b --factory-startup --python-exit-code 1 --python tools/rig_stalker.py -- \
      assets/models/stalker/uhd/stalker.glb assets/models/stalker/uhd/stalker_rigged.glb \
      --yaw-degrees -90 --height 0.977325 --t-pose --coat --weights proximity \
      --image-format AUTO
  python3 tools/compress_glb_textures.py assets/models/stalker/uhd/stalker_rigged.glb
(the compression step must run last; Blender cannot read the KTX2 output)

Blender coords: Z up, model front faces -Y, character's left is +X (glTF: front +Z).
  --yaw-degrees   rotate the import about Z into that convention before fitting joints.
  --height        ground the feet at z=0 and scale uniformly to this height (m).
  --t-pose        the bind pose is outstretched; arms are fitted along their length and
                  the clips hang them from the shoulders. The mesh is not reposed.
  --coat          add CoatFront/Back/Left/Right .1 and .2 chains under the hips that
                  carry the trench-coat skirt. They stay in rest pose in every clip.
  --weights       heat (Blender bone heat, medium) or proximity (distance to bone
                  segments, for ~1M-triangle meshes where heat is too slow).
  --weights-from  a lighter scan of the same model: bone heat runs on it and the
                  weights are copied onto the full mesh. Use it when the arms hang
                  against the body, where proximity gives the torso's sides to the arms.
  --image-format  JPEG re-encodes at quality 88; AUTO keeps the source image bytes.
  --walk-arms     the Walk clip's arm swing at the shoulder, in degrees (default 18); the
                  elbow bend grows in proportion.
  --armature      the source is already rigged (a Tripo biped rig): keep its skeleton and
                  weights and bake the clips onto it through TRIPO_BONES (or HUMANOID_BONES).
  --center        shift the scan so its head sits on x = 0, where the spine is fitted (a
                  raised arm pulls the bounds off the body).
  --joints        a JSON file of arm joints per side ({"L": {"shoulder", "elbow", "wrist",
                  "hand_end"}, "R": ...}, fractions of the height in the axes above) for a
                  pose the slice fit cannot read; a side left out is fitted and mirrored.
  --hang-arms     the bind pose's arms are bent (a gesture): the clips turn every arm bone
                  to hang straight down, as --t-pose does for outstretched arms.
"""
import argparse
import json
import math
import sys

import bpy
import numpy as np
from mathutils import Matrix, Quaternion, Vector


def parse_args():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument('src')
    p.add_argument('dst')
    p.add_argument('--yaw-degrees', type=float, default=0.0)
    p.add_argument('--height', type=float)
    p.add_argument('--t-pose', action='store_true')
    p.add_argument('--coat', action='store_true')
    p.add_argument('--weights', choices=('heat', 'proximity'), default='heat')
    p.add_argument('--weights-from')
    p.add_argument('--image-format', choices=('JPEG', 'AUTO'), default='JPEG')
    p.add_argument('--walk-arms', type=float, default=18.0)
    p.add_argument('--armature', action='store_true')
    p.add_argument('--center', action='store_true')
    p.add_argument('--joints')
    p.add_argument('--hang-arms', action='store_true')
    args = p.parse_args(sys.argv[sys.argv.index('--') + 1:])
    if args.armature and (args.joints or args.hang_arms or args.center):
        p.error('--armature keeps the source\'s own joints')
    if args.hang_arms and args.t_pose:
        p.error('--t-pose already hangs the arms')
    if args.coat and args.weights != 'proximity':
        p.error('--coat needs --weights proximity')
    if args.weights_from and args.weights != 'heat':
        p.error('--weights-from computes heat weights')
    if args.armature and (args.coat or args.weights_from):
        p.error('--armature keeps the source\'s own skeleton and weights')
    return args


A = parse_args()
FPS = 30
bpy.ops.wm.read_factory_settings(use_empty=True)
sc = bpy.context.scene
sc.render.fps = FPS


def vertex_array(obj):
    co = np.empty(len(obj.data.vertices) * 3, dtype=np.float64)
    obj.data.vertices.foreach_get('co', co)
    return co.reshape(-1, 3)


def apply_transform(obj):
    bpy.ops.object.select_all(action='DESELECT')
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)


def import_scan(path):
    """Import a scan's mesh with its transforms applied, turned by --yaw-degrees and
    grounded at --height."""
    before = set(sc.objects)
    bpy.ops.import_scene.gltf(filepath=path)
    imported = [o for o in sc.objects if o not in before]
    obj = next(o for o in imported if o.type == 'MESH')
    bpy.ops.object.select_all(action='DESELECT')
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.parent_clear(type='CLEAR_KEEP_TRANSFORM')
    apply_transform(obj)
    for o in imported:
        if o is not obj:
            bpy.data.objects.remove(o)
    if A.yaw_degrees:
        obj.rotation_mode = 'XYZ'  # glTF imports use quaternions, which would ignore rotation_euler
        obj.rotation_euler.z = math.radians(A.yaw_degrees)
        apply_transform(obj)
    if A.center:
        # The rig's spine runs up x = 0; a raised arm pulls the bounds off the body,
        # so center the head (the top 8 % of the scan) instead.
        v = vertex_array(obj)
        head = v[v[:, 2] > v[:, 2].max() - 0.08 * (v[:, 2].max() - v[:, 2].min()), 0]
        obj.location.x = -(head.min() + head.max()) / 2
        apply_transform(obj)
    if A.height:
        z = vertex_array(obj)[:, 2]
        s = A.height / (z.max() - z.min())
        obj.scale = (s, s, s)
        obj.location.z = -z.min() * s
        apply_transform(obj)
    return obj


def fit_rig():
    """Fit a skeleton to the scan from slices of its mesh and skin it."""
    mesh = import_scan(A.src)
    V = vertex_array(mesh)
    H = V[:, 2].max()


    def center(zlo, zhi, xlo=-9, xhi=9):
        """Bounding-box center (x, y) of the mesh slice in a height/side band."""
        x, y, z = V[:, 0], V[:, 1], V[:, 2]
        m = (zlo * H <= z) & (z <= zhi * H) & (xlo * H <= x) & (x <= xhi * H)
        return Vector(((x[m].min() + x[m].max()) / 2, (y[m].min() + y[m].max()) / 2))


    def at(c, z):
        return Vector((c.x, c.y, z * H))


    body = center(0.45, 0.80, -0.12, 0.12)
    spine = lambda z: Vector((0, body.y, z * H))


    def arm_joint(frac):
        """Centroid of the outstretched left arm at a fraction of the height along +X."""
        x, y, z = V[:, 0], V[:, 1], V[:, 2]
        xk = frac * H
        m = (np.abs(x - xk) < 0.01 * H) & (z >= 0.72 * H) & (z <= 0.82 * H) & (np.abs(y) < 0.25 * H)
        return Vector((xk, y[m].mean(), z[m].mean()))


    if A.t_pose:
        shoulder, elbow = arm_joint(0.175), arm_joint(0.31)
        wrist, hand_end = arm_joint(0.40), arm_joint(0.44)
    else:
        # Limb joints: fitted from slices of the mesh on the character's left (+X).
        shoulder = at(center(0.74, 0.78, 0.10, 0.20), 0.775)
        elbow = at(center(0.61, 0.65, 0.13, 0.30), 0.63)
        wrist = at(center(0.47, 0.50, 0.13, 0.30), 0.485)
        hand_end = at(center(0.40, 0.44, 0.13, 0.30), 0.41)
    ankle = at(center(0.08, 0.11, 0.0, 0.15), 0.085)
    if A.coat:
        # A robe hides the thighs and knees: the slices there are dominated by the skirt, which
        # biases their centers ~5 cm forward. Fit the visible shins below the hem and extend
        # that axis up to the knee and hip.
        shin = at(center(0.13, 0.19, 0.0, 0.15), 0.16)

        def along(z):
            t = (z * H - ankle.z) / (shin.z - ankle.z)
            return ankle + (shin - ankle) * t

        knee, hip = along(0.28), along(0.50)
    else:
        hip = at(center(0.40, 0.44, 0.01, 0.15), 0.50)
        knee = at(center(0.26, 0.30, 0.0, 0.15), 0.28)
    low = (V[:, 2] < 0.05 * H) & (V[:, 0] > 0)
    toe = Vector((ankle.x, V[low, 1].min(), 0.015 * H))
    print('JOINTS', dict(shoulder=shoulder, elbow=elbow, wrist=wrist, hip=hip, knee=knee, ankle=ankle, toe=toe))

    arm_data = bpy.data.armatures.new('Rig')
    rig = bpy.data.objects.new('Stalker', arm_data)
    sc.collection.objects.link(rig)
    bpy.context.view_layer.objects.active = rig
    bpy.ops.object.mode_set(mode='EDIT')
    eb = arm_data.edit_bones
    segments = {}


    def bone(name, head, tail, parent=None, connect=False):
        b = eb.new(name)
        b.head, b.tail = head, tail
        if parent:
            b.parent = eb[parent]
            b.use_connect = connect
        segments[name] = (head, tail)
        return b


    bone('hips', spine(0.50), spine(0.58))
    bone('spine', spine(0.58), spine(0.68), 'hips', True)
    bone('chest', spine(0.68), spine(0.79), 'spine', True)
    bone('neck', spine(0.79), spine(0.84), 'chest', True)
    bone('head', spine(0.84), spine(0.99), 'neck', True)
    given = json.load(open(A.joints)) if A.joints else {}

    def arm(side, m):
        """The arm's joints: as --joints gives them (fractions of the height), or the
        fitted left arm mirrored."""
        if side in given:
            return [Vector(given[side][k]) * H for k in ('shoulder', 'elbow', 'wrist', 'hand_end')]
        return [Vector((v.x * m, v.y, v.z)) for v in (shoulder, elbow, wrist, hand_end)]

    for side, m in (('L', 1), ('R', -1)):
        f = lambda v: Vector((v.x * m, v.y, v.z))
        s, e, w, h = arm(side, m)
        bone(f'shoulder.{side}', spine(0.775) + Vector((0.03 * m, 0, 0)), s, 'chest')
        bone(f'upper_arm.{side}', s, e, f'shoulder.{side}', True)
        bone(f'forearm.{side}', e, w, f'upper_arm.{side}', True)
        bone(f'hand.{side}', w, h, f'forearm.{side}', True)
        bone(f'thigh.{side}', f(hip), f(knee), 'hips')
        bone(f'shin.{side}', f(knee), f(ankle), f'thigh.{side}', True)
        bone(f'foot.{side}', f(ankle), f(toe), f'shin.{side}', True)


    def coat_point(side, z):
        """Coat surface on one side at height z, pulled 20 % inside so the bone sits in cloth."""
        zf = z * H
        band = np.abs(V[:, 2] - zf) < 0.03 * H
        if side == 'Front':
            s = V[band & (np.abs(V[:, 0]) < 0.12 * H)]
            return Vector((0, 0.8 * s[:, 1].min(), zf))
        if side == 'Back':
            s = V[band & (np.abs(V[:, 0]) < 0.12 * H)]
            return Vector((0, 0.8 * s[:, 1].max(), zf))
        s = V[band & (np.abs(V[:, 1] - body.y) < 0.05 * H)]
        if side == 'Left':
            return Vector((0.8 * s[:, 0].max(), body.y, zf))
        return Vector((0.8 * s[:, 0].min(), body.y, zf))


    if A.coat:
        for side in ('Front', 'Back', 'Left', 'Right'):
            p0, p1, p2 = (coat_point(side, z) for z in (0.50, 0.38, 0.24))
            bone(f'Coat{side}.1', p0, p1, 'hips')
            bone(f'Coat{side}.2', p1, p2, f'Coat{side}.1', True)
    for b in eb:
        b.roll = 0
    bpy.ops.object.mode_set(mode='OBJECT')


    def heat_weights(mesh):
        """Blender bone-heat weights, computed on a welded copy scaled 10x (small meshes only)."""
        bpy.ops.object.select_all(action='DESELECT')
        mesh.select_set(True)
        rig.select_set(True)
        bpy.context.view_layer.objects.active = mesh
        bpy.ops.object.mode_set(mode='EDIT')
        bpy.ops.mesh.select_all(action='SELECT')
        bpy.ops.mesh.remove_doubles(threshold=0.0005)
        bpy.ops.object.mode_set(mode='OBJECT')
        for o in (mesh, rig):
            o.scale = (10, 10, 10)
        bpy.ops.object.select_all(action='DESELECT')
        for o in (mesh, rig):
            o.select_set(True)
        bpy.context.view_layer.objects.active = rig
        bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
        bpy.ops.object.parent_set(type='ARMATURE_AUTO')
        bpy.ops.object.select_all(action='DESELECT')
        rig.select_set(True)
        bpy.context.view_layer.objects.active = rig
        rig.scale = (0.1, 0.1, 0.1)
        mesh.select_set(True)
        bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
        # Copy weights onto the few vertices bone heat missed (Bevy would collapse them).
        from mathutils import kdtree
        mv = mesh.data.vertices
        done = [v for v in mv if any(g.weight > 1e-3 for g in v.groups)]
        tree = kdtree.KDTree(len(done))
        for i, v in enumerate(done):
            tree.insert(v.co, i)
        tree.balance()
        for v in mv:
            if not any(g.weight > 1e-3 for g in v.groups):
                src = done[tree.find(v.co)[1]]
                for g in src.groups:
                    mesh.vertex_groups[g.group].add([v.index], g.weight, 'REPLACE')
        unweighted = sum(1 for v in mesh.data.vertices if not any(g.weight > 1e-3 for g in v.groups))
        print('UNWEIGHTED', unweighted, 'of', len(mesh.data.vertices))


    def bone_sigma(name):
        if name.startswith('Coat'):
            return 0.06
        return SIGMA[name.rsplit('.', 1)[0]]


    SIGMA = {'hips': 0.08, 'spine': 0.08, 'chest': 0.09, 'neck': 0.05, 'head': 0.10,
             'shoulder': 0.05, 'upper_arm': 0.05, 'forearm': 0.045, 'hand': 0.04,
             'thigh': 0.065, 'shin': 0.055, 'foot': 0.045}


    def segment_distance(P, a, b):
        a = np.array(a, dtype=np.float64)
        ab = np.array(b, dtype=np.float64) - a
        t = np.clip((P - a) @ ab / (ab @ ab), 0.0, 1.0)
        return np.linalg.norm(P - (a + t[:, None] * ab), axis=1)


    def soft_nearest(D, sig):
        """Gaussian falloff of each bone's distance relative to the nearest bone, normalized."""
        w = np.exp(-0.5 * ((D - D.min(1, keepdims=True)) / sig) ** 2)
        return w / w.sum(1, keepdims=True)


    def smoothstep(e0, e1, x):
        t = np.clip((x - e0) / (e1 - e0), 0.0, 1.0)
        return t * t * (3 - 2 * t)


    def proximity_weights():
        """Skin by distance to each bone segment; the coat skirt moves to the coat chains.

        Joints blend smoothly because each bone's falloff is relative to the nearest bone.
        The skirt is taken over only outside the legs (between the thigh and shin axes
        and the cloth) and between the hem and the waist, so boots and legs keep their bones.
        """
        names = list(segments)
        D = np.stack([segment_distance(V, *segments[n]) for n in names], 1)
        sig = np.array([bone_sigma(n) for n in names])
        coat = np.array([n.startswith('Coat') for n in names])
        leg = np.array([n.split('.')[0] in ('thigh', 'shin') for n in names])
        W = np.zeros_like(D)
        body_cols = ~coat
        W[:, body_cols] = soft_nearest(D[:, body_cols], sig[body_cols])
        if coat.any():
            zf = V[:, 2] / H
            leg_d = D[:, leg].min(1)
            s = smoothstep(0.52, 0.46, zf) * smoothstep(0.20, 0.26, zf) * smoothstep(0.07, 0.11, leg_d)
            W[:, body_cols] *= (1 - s)[:, None]
            W[:, coat] = s[:, None] * soft_nearest(D[:, coat], sig[coat])
            print('COAT VERTS', int((s > 0.5).sum()), 'partially coat', int(((s > 0.01) & (s <= 0.5)).sum()))
        # Bevy and glTF skinning take four influences per vertex.
        top = np.argsort(-W, axis=1)[:, :4]
        keep = np.zeros_like(W)
        np.put_along_axis(keep, top, np.take_along_axis(W, top, 1), 1)
        keep /= keep.sum(1, keepdims=True)
        return names, keep


    def apply_proximity_weights():
        names, W = proximity_weights()
        for j, name in enumerate(names):
            idx = np.nonzero(W[:, j] > 0)[0]
            if len(idx) == 0:
                continue
            group = mesh.vertex_groups.new(name=name)
            for i, w in zip(idx.tolist(), W[idx, j].tolist()):  # add() takes one weight at a time
                group.add([i], w, 'REPLACE')
        print('WEIGHTED', len(V), 'vertices across', len(names), 'bones')
        mod = mesh.modifiers.new('Armature', 'ARMATURE')
        mod.object = rig
        mesh.parent = rig


    def transfer_heat_weights():
        """Bone heat on the lighter scan, copied onto the nearest surface of the full mesh.

        Heat only reaches skin that can see the bone, so the torso beside a hanging arm
        stays with the torso. Bone heat is too slow for the full mesh itself.
        """
        proxy = import_scan(A.weights_from)
        heat_weights(proxy)
        for group in proxy.vertex_groups:
            mesh.vertex_groups.new(name=group.name)
        copy = mesh.modifiers.new('Weights', 'DATA_TRANSFER')
        copy.object = proxy
        copy.use_vert_data = True
        copy.data_types_verts = {'VGROUP_WEIGHTS'}
        copy.vert_mapping = 'POLYINTERP_NEAREST'
        copy.layers_vgroup_select_src = 'ALL'
        copy.layers_vgroup_select_dst = 'NAME'
        bpy.ops.object.select_all(action='DESELECT')
        bpy.context.view_layer.objects.active = mesh
        bpy.ops.object.modifier_apply(modifier=copy.name)
        bpy.data.objects.remove(proxy)
        mod = mesh.modifiers.new('Armature', 'ARMATURE')
        mod.object = rig
        mesh.parent = rig
        print('TRANSFERRED', len(mesh.vertex_groups), 'bone weights onto', len(mesh.data.vertices), 'vertices')


    if A.weights_from:
        transfer_heat_weights()
    elif A.weights == 'heat':
        heat_weights(mesh)
    else:
        apply_proximity_weights()
    return rig, mesh


def import_rig():
    """Import an already rigged scan (a Tripo rig) with its skeleton and weights,
    turned by --yaw-degrees and grounded at --height like an unrigged one."""
    before = set(sc.objects)
    bpy.ops.import_scene.gltf(filepath=A.src)
    imported = [o for o in sc.objects if o not in before]
    rig = next(o for o in imported if o.type == 'ARMATURE')
    mesh = next(o for o in imported if o.type == 'MESH' and o.parent is rig)
    for o in imported:
        if o not in (rig, mesh):
            bpy.data.objects.remove(o)  # the importer's bone display shape
    rig.rotation_mode = 'XYZ'
    rig.rotation_euler.z = math.radians(A.yaw_degrees)
    if A.height:
        bpy.context.view_layer.update()
        z = np.array([(mesh.matrix_world @ v.co).z for v in mesh.data.vertices])
        s = A.height / (z.max() - z.min())
        rig.scale = (s, s, s)
        rig.location.z = -z.min() * s
    bpy.ops.object.select_all(action='DESELECT')
    for o in (rig, mesh):
        o.select_set(True)
    bpy.context.view_layer.objects.active = rig
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
    return rig, mesh


rig, mesh = import_rig() if A.armature else fit_rig()
arm_data = rig.data

# --- Animation -------------------------------------------------------------
# The clips name the fitted rig's bones; a Tripo biped rig calls them this.
TRIPO_BONES = {'hips': 'Hip', 'spine': 'Spine01', 'chest': 'Spine02', 'neck': 'NeckTwist01',
               'head': 'Head'}
for _side in ('L', 'R'):
    TRIPO_BONES.update({
        f'shoulder.{_side}': f'{_side}_Clavicle', f'upper_arm.{_side}': f'{_side}_Upperarm',
        f'forearm.{_side}': f'{_side}_Forearm', f'hand.{_side}': f'{_side}_Hand',
        f'thigh.{_side}': f'{_side}_Thigh', f'shin.{_side}': f'{_side}_Calf',
        f'foot.{_side}': f'{_side}_Foot',
    })
# Newer Tripo rigs name the bones like a Unity humanoid.
HUMANOID_BONES = {'hips': 'Hips', 'spine': 'Spine', 'chest': 'Chest', 'neck': 'Neck',
                  'head': 'Head'}
for _side, _long in (('L', 'Left'), ('R', 'Right')):
    HUMANOID_BONES.update({
        f'shoulder.{_side}': f'{_long}_Shoulder', f'upper_arm.{_side}': f'{_long}_UpperArm',
        f'forearm.{_side}': f'{_long}_LowerArm', f'hand.{_side}': f'{_long}_Hand',
        f'thigh.{_side}': f'{_long}_UpperLeg', f'shin.{_side}': f'{_long}_LowerLeg',
        f'foot.{_side}': f'{_long}_Foot',
    })
if not A.armature:
    BONE = {}
elif 'Hips' in arm_data.bones:
    BONE = HUMANOID_BONES
else:
    BONE = TRIPO_BONES


def bone_name(name):
    return BONE.get(name, name)


bpy.context.view_layer.update()
bones = arm_data.bones
rest = {b.name: b.matrix_local.to_3x3() for b in bones}  # armature-space rest rotations
parent_of = {b.name: (b.parent.name if b.parent else None) for b in bones}


def depth(name):
    d, p = 0, parent_of[name]
    while p:
        d, p = d + 1, parent_of[p]
    return d


order = sorted(rest, key=depth)
# Outstretched arms hang for the clips: the hang rotates the T-pose rest of the upper
# arm and every bone below it (forearm, hand, twist and finger bones) about Y.
hang = {}
HANG_OUT = 6.0  # degrees out from the body for --hang-arms
if A.t_pose:
    for side, m in (('L', 1), ('R', -1)):
        upper = bone_name(f'upper_arm.{side}')
        for name in rest:
            p = name
            while p and p != upper:
                p = parent_of[p]
            if p == upper:
                hang[name] = Matrix.Rotation(math.radians(90 * m), 3, 'Y')
elif A.hang_arms:
    # A bent bind pose (a gesture): every arm bone turns by the least rotation from its
    # rest direction to hanging down, a little out from the body so it clears the hips.
    for side, m in (('L', 1), ('R', -1)):
        down = Vector((math.sin(math.radians(HANG_OUT)) * m, 0, -math.cos(math.radians(HANG_OUT))))
        for part in ('upper_arm', 'forearm', 'hand'):
            name = bone_name(f'{part}.{side}')
            hang[name] = (rest[name] @ Vector((0, 1, 0))).rotation_difference(down).to_matrix()


def hanging_rest(name):
    return hang.get(name, Matrix.Identity(3)) @ rest[name]


def solve(delta):
    """Pose quaternions that realize world-axis deltas on top of the hanging rest.

    Each bone's world orientation is parent_world * hanging_parent^-1 * delta * hanging_rest,
    so a delta on the shoulder carries the arm, and the local pose is recovered from it.
    Bones without a delta stay in the (hanging) rest pose.
    """
    G, quats = {}, {}
    for name in order:
        Rb, Hb = rest[name], hanging_rest(name)
        p = parent_of[name]
        if p is None:
            Rp = Gp = Hp = Matrix.Identity(3)
        else:
            Rp, Hp, Gp = rest[p], hanging_rest(p), G[p]
        G[name] = Gp @ Hp.inverted() @ delta.get(name, Matrix.Identity(3)) @ Hb
        quats[name] = (Rb.inverted() @ Rp @ Gp.inverted() @ G[name]).to_quaternion()
    return quats


def rot(x=0.0, y=0.0, z=0.0):
    """Delta rotation in armature axes (degrees).

    +X swings a hanging limb backward (front is -Y), -X forward; it tips an
    upright bone (spine, head) forward, -X back.
    """
    return (Matrix.Rotation(math.radians(z), 3, 'Z')
            @ Matrix.Rotation(math.radians(y), 3, 'Y')
            @ Matrix.Rotation(math.radians(x), 3, 'X'))


def key(action_name, frames, pose_at, loop=True):
    rig.animation_data_create()
    act = bpy.data.actions.new(action_name)
    act.use_fake_user = True
    rig.animation_data.action = act
    for f in range(frames + 1):
        t = (f % frames if loop else f) / frames
        pose = pose_at(t)
        quats = solve({bone_name(k): v for k, v in pose.items() if not k.startswith('_')})
        for pb in rig.pose.bones:
            if pb.name.startswith('Coat'):
                continue  # unkeyed: clips leave the coat at rest and the game drives it
            pb.rotation_mode = 'QUATERNION'
            pb.rotation_quaternion = quats[pb.name]
            pb.location = Vector()
            pb.keyframe_insert('rotation_quaternion', frame=f)
        hips = rig.pose.bones[bone_name('hips')]
        # The lift is along world up, in the hip bone's own rest axes.
        hips.location = rest[hips.name].inverted() @ Vector((0, 0, pose.get('_lift', 0.0)))
        hips.keyframe_insert('location', frame=f)
    # Keys sit on whole frames, so linear keys equal the sampled values exactly.
    for fc in act.fcurves:
        for kp in fc.keyframe_points:
            kp.interpolation = 'LINEAR'
    rig.animation_data.action = None


TAU = math.tau


# The scans stand with their feet wider than their hips; in stride the legs swing in
# under the body, so the feet pass close to each other, and the soles stay level.
ADDUCT = 7.0


def gait(t, stride, knee_max, arm, elbow, lean, bob, twist):
    p = TAU * t
    q = {}
    for side, ph, m in (('L', 0.0, 1), ('R', math.pi, -1)):
        fwd = math.sin(p + ph)                  # >0: leg ahead of the body
        swing = max(0.0, math.cos(p + ph))      # leg moving forward
        knee = 6 + knee_max * swing ** 1.5
        q[f'thigh.{side}'] = rot(x=-stride * fwd - 0.3 * knee * swing, y=m * ADDUCT)
        q[f'shin.{side}'] = rot(x=knee)
        q[f'foot.{side}'] = rot(x=-0.35 * knee + 8 * fwd, y=-m * ADDUCT)
        q[f'upper_arm.{side}'] = rot(x=arm * fwd)  # opposite leg
        q[f'forearm.{side}'] = rot(x=-elbow - 0.4 * elbow * max(0.0, -fwd))
    q['spine'] = rot(x=lean * 0.5, z=twist * math.sin(p))
    q['chest'] = rot(x=lean * 0.5, z=-1.5 * twist * math.sin(p))
    q['head'] = rot(x=-lean * 0.6)
    q['_lift'] = -bob * math.sin(p) ** 2
    return q


key('Walk', 30, lambda t: gait(t, stride=24, knee_max=40, arm=A.walk_arms, elbow=A.walk_arms * 2 / 3,
                                lean=3, bob=0.010, twist=4))
key('Run', 20, lambda t: gait(t, stride=40, knee_max=85, arm=40, elbow=75, lean=12, bob=0.022, twist=7))


def idle(t):
    b = math.sin(TAU * t)
    return {
        'chest': rot(x=-1.5 * b),
        'head': rot(x=1.0 * b, z=2.0 * math.sin(TAU * t * 0.5 + 1)),
        'upper_arm.L': rot(x=1.5 * b),
        'upper_arm.R': rot(x=1.5 * b),
        'forearm.L': rot(x=-8), 'forearm.R': rot(x=-8),
        '_lift': -0.002 * (1 + b),
    }


key('Idle', 90, idle)


def jump(t):
    k = min(1.0, t / 0.3)                       # tuck in over the first 30 %
    s = k * k * (3 - 2 * k)
    q = {}
    for side, m in (('L', 1), ('R', -1)):
        lead = 1.0 if side == 'L' else 0.6
        q[f'thigh.{side}'] = rot(x=-55 * lead * s)
        q[f'shin.{side}'] = rot(x=80 * lead * s)
        q[f'foot.{side}'] = rot(x=-15 * s)
        q[f'upper_arm.{side}'] = rot(x=-50 * s, y=-15 * m * s)
        q[f'forearm.{side}'] = rot(x=-50 * s)
    q['spine'] = rot(x=8 * s)
    q['head'] = rot(x=-6 * s)
    return q


key('Jump', 15, jump, loop=False)


def crawl(t):
    """Commando crawl, posed standing: the game pitches the body face-down.

    "Overhead" is ahead along the ground; each arm reaches while the leg on
    the same side straightens and the opposite knee draws up the side.
    """
    p = TAU * t
    q = {}
    for side, ph, m in (('L', 0.0, 1), ('R', math.pi, -1)):
        reach = math.sin(p + ph)                # 1: arm stretched ahead
        draw = 0.5 * (1 - reach)                # 1: knee drawn up
        q[f'upper_arm.{side}'] = rot(x=-(115 + 35 * reach), y=-m * 20)
        q[f'forearm.{side}'] = rot(x=-(70 - 45 * reach))
        q[f'thigh.{side}'] = rot(x=-40 * draw, y=-m * (8 + 30 * draw))
        q[f'shin.{side}'] = rot(x=5 + 80 * draw)
    q['spine'] = rot(x=-4, z=5 * math.sin(p))
    q['chest'] = rot(z=-5 * math.sin(p))
    q['neck'] = rot(x=-20)
    q['head'] = rot(x=-35)
    return q


key('Crawl', 40, crawl)

# Rest pose as the default: the bind pose (outstretched arms in --t-pose builds).
for pb in rig.pose.bones:
    pb.rotation_quaternion = Quaternion()
    pb.location = Vector()

# Bake tangents for normal maps, or Bevy generates them for every vertex at load.
normal_mapped = any(
    node.type == 'NORMAL_MAP'
    for slot in mesh.material_slots if slot.material and slot.material.node_tree
    for node in slot.material.node_tree.nodes
)
bpy.ops.export_scene.gltf(
    filepath=A.dst, export_format='GLB', export_animation_mode='ACTIONS',
    export_tangents=normal_mapped,
    export_image_format=A.image_format, export_jpeg_quality=88, export_apply=False,
    # Sampling writes a channel for every bone; without it only keyed bones (not the coat) get one.
    export_force_sampling=not A.coat, export_optimize_animation_size=False,
    export_anim_slide_to_zero=True,
)
print('EXPORTED', A.dst)
