"""Precompile the textures embedded in GLBs to GPU block-compressed KTX2 with full mip chains.

Color maps become sRGB BC7, normal maps two-channel BC5 and packed data maps (metallic-roughness,
occlusion) linear BC7, all zstd-supercompressed. Bevy reads embedded `image/ktx2` buffers directly
(the `ktx2` and `zstd_rust` features), so the game skips PNG/JPEG decoding and CPU mip generation,
and the textures take a quarter of the VRAM and bandwidth.

Run it as the last step of an asset's pipeline: Blender and glTF Transform cannot read the converted
textures. Images that are already KTX2 are left alone, so re-running is safe.

    python3 tools/compress_glb_textures.py assets/models/base/hangar.glb [more.glb ...]

With --external-images DIR the textures are written as .ktx2 files in DIR instead, each GLB's image
referring to its file by a relative `uri`. GLBs that share a texture (identical image bytes in the
same run) then share one file, and Bevy, which loads a URI image by its asset path, keeps one copy in
memory for all of them. A file is named after its glTF image (the GLB's name and the image's index
when it has none); an image that is already KTX2 is moved out as it is.

    python3 tools/compress_glb_textures.py --external-images assets/models/vehicles/medium/textures \
        assets/models/vehicles/medium/{ambulance,fire-truck,panel-van}-traffic.glb

Requires the ctt CLI (the encoder behind Bevy's CompressedImageSaver): cargo install ctt-cli
"""

import argparse
import hashlib
import json
import os
import re
import struct
import subprocess
import tempfile
from pathlib import Path

GLB_MAGIC, JSON_CHUNK, BIN_CHUNK = 0x46546C67, 0x4E4F534A, 0x004E4942
COLOR_SLOTS = {"baseColorTexture", "emissiveTexture"}
# Extensions that point at buffer views themselves; moving images out would have to renumber them.
VIEW_EXTENSIONS = {"EXT_meshopt_compression", "KHR_draco_mesh_compression"}


def read_glb(path):
    data = path.read_bytes()
    magic, _, _ = struct.unpack_from("<III", data)
    if magic != GLB_MAGIC:
        raise ValueError(f"{path} is not a GLB")
    json_length, _ = struct.unpack_from("<II", data, 12)
    gltf = json.loads(data[20 : 20 + json_length])
    bin_offset = 20 + json_length
    bin_length, chunk_type = struct.unpack_from("<II", data, bin_offset)
    if chunk_type != BIN_CHUNK or len(gltf.get("buffers", [])) != 1:
        raise ValueError(f"{path} must have exactly one embedded buffer")
    return gltf, data[bin_offset + 8 : bin_offset + 8 + bin_length]


def write_glb(path, gltf, binary):
    text = json.dumps(gltf, separators=(",", ":")).encode()
    text += b" " * (-len(text) % 4)
    binary += b"\0" * (-len(binary) % 4)
    total = 12 + 8 + len(text) + 8 + len(binary)
    path.write_bytes(
        struct.pack("<III", GLB_MAGIC, 2, total)
        + struct.pack("<II", len(text), JSON_CHUNK)
        + text
        + struct.pack("<II", len(binary), BIN_CHUNK)
        + binary
    )


def texture_slots(node):
    """Yields (slot, texture index) for every `*Texture` reference, including extensions."""
    if isinstance(node, dict):
        for key, value in node.items():
            if key.endswith("Texture") and isinstance(value, dict) and "index" in value:
                yield key, value["index"]
            else:
                yield from texture_slots(value)
    elif isinstance(node, list):
        for value in node:
            yield from texture_slots(value)


def image_roles(gltf):
    roles = {}
    for slot, texture in texture_slots(gltf.get("materials", [])):
        source = gltf["textures"][texture].get("source")
        if source is None:
            continue
        role = "normal" if slot == "normalTexture" else "color" if slot in COLOR_SLOTS else "data"
        roles.setdefault(source, set()).add(role)
    return roles


def ctt_arguments(role):
    if role == "color":
        return ["-f", "bc7", "--ic", "srgb", "--mipmap", "--mipmap-filter", "lanczos3"]
    if role == "normal":
        return ["-f", "bc5", "--ic", "linear", "--mipmap", "--allow-discarding-alpha"]
    if role == "depth":
        # One channel (a relief map's red): BC4 is half BC7's size and more precise.
        return ["-f", "bc4", "--ic", "linear", "--mipmap", "--mipmap-filter", "lanczos3"]
    return [
        "-f", "bc7", "--ic", "linear", "--mipmap", "--mipmap-filter", "lanczos3",
        "--bc7e-opts", "perceptual=false",
    ]


def write_external(ktx2, name, directory, glb, written):
    """Write the texture as <name>.ktx2 in `directory` (once per run for the same bytes) and return
    its URI relative to the GLB. A different texture with the same name gets its hash appended."""
    digest = hashlib.sha256(ktx2).hexdigest()
    stem = re.sub(r"[^A-Za-z0-9_-]+", "-", name)
    target = directory / f"{stem}.ktx2"
    if written.get(target, digest) != digest:
        target = directory / f"{stem}-{digest[:8]}.ktx2"
    if target not in written:
        directory.mkdir(parents=True, exist_ok=True)
        target.write_bytes(ktx2)
        written[target] = digest
    return Path(os.path.relpath(target, glb.parent)).as_posix()


def compress(image_bytes, mime_type, role, quality, scratch, cache=None):
    """Encode one image; with a `cache` dict, identical images in a run are encoded once."""
    key = (hashlib.sha256(image_bytes).hexdigest(), role, quality)
    if cache is not None and key in cache:
        return cache[key]
    source = scratch / ("source.png" if mime_type == "image/png" else "source.jpg")
    target = scratch / "target.ktx2"
    source.write_bytes(image_bytes)
    subprocess.run(
        ["ctt", str(source), "-o", str(target), "--quality", quality, "--zstd=19"]
        + ctt_arguments(role),
        check=True,
        capture_output=True,
    )
    encoded = target.read_bytes()
    if cache is not None:
        cache[key] = encoded
    return encoded


def convert(path, quality, external=None, encoded=None, written=None):
    """Compress the GLB's embedded images in place, or with `external` (a directory) move them
    out as .ktx2 files. `encoded` caches this run's encodings by source image; `written` maps each
    file written this run to its bytes' hash."""
    gltf, binary = read_glb(path)
    roles = image_roles(gltf)
    if external and VIEW_EXTENSIONS & set(gltf.get("extensionsUsed", [])):
        raise ValueError(f"{path}: --external-images cannot renumber {sorted(VIEW_EXTENSIONS)} views")
    replaced = {}
    with tempfile.TemporaryDirectory() as scratch:
        for index, image in enumerate(gltf.get("images", [])):
            if index not in roles or "bufferView" not in image:
                continue
            if image.get("mimeType") == "image/ktx2" and not external:
                continue
            if len(roles[index]) > 1:
                raise ValueError(f"{path}: image {index} is shared by {sorted(roles[index])} slots")
            view = gltf["bufferViews"][image["bufferView"]]
            start = view.get("byteOffset", 0)
            source = binary[start : start + view["byteLength"]]
            ktx2 = source
            if image.get("mimeType") != "image/ktx2":
                role = next(iter(roles[index]))
                ktx2 = compress(source, image.get("mimeType"), role, quality, Path(scratch), encoded)
            if external:
                name = image.get("name") or f"{path.stem}-{index}"
                replaced[image.pop("bufferView")] = None
                image["uri"] = write_external(ktx2, name, external, path, written)
            else:
                replaced[image["bufferView"]] = ktx2
            image["mimeType"] = "image/ktx2"
    if not replaced:
        return 0, 0

    # Repack every view so the new image sizes keep the others 8-byte aligned; views of images
    # moved out are dropped and the rest renumbered.
    packed = bytearray()
    kept, renumber = [], {}
    for index, view in enumerate(gltf["bufferViews"]):
        start = view.get("byteOffset", 0)
        chunk = replaced.get(index, binary[start : start + view["byteLength"]])
        if chunk is None:
            continue
        packed += b"\0" * (-len(packed) % 8)
        view["byteOffset"] = len(packed)
        view["byteLength"] = len(chunk)
        packed += chunk
        renumber[index] = len(kept)
        kept.append(view)
    gltf["bufferViews"] = kept
    for user in [*gltf.get("accessors", []), *gltf.get("images", [])]:
        sparse = user.get("sparse", {})
        for holder in [user, sparse.get("indices", {}), sparse.get("values", {})]:
            if "bufferView" in holder:
                holder["bufferView"] = renumber[holder["bufferView"]]
    gltf["buffers"][0]["byteLength"] = len(packed)
    before = path.stat().st_size
    write_glb(path, gltf, bytes(packed))
    return before, path.stat().st_size


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("glb", nargs="+", type=Path)
    parser.add_argument("--quality", default="basic", help="ctt quality preset (default: basic)")
    parser.add_argument(
        "--external-images",
        type=Path,
        metavar="DIR",
        help="write the textures as .ktx2 files in DIR, shared between the GLBs, instead of embedding them",
    )
    args = parser.parse_args()
    encoded, written = {}, {}
    for path in args.glb:
        before, after = convert(path, args.quality, args.external_images, encoded, written)
        if before:
            print(f"{path}: {before / 1e6:.1f} MB -> {after / 1e6:.1f} MB")
        else:
            print(f"{path}: already compressed")


if __name__ == "__main__":
    main()
