"""Precompile the textures embedded in GLBs to GPU block-compressed KTX2 with full mip chains.

Color maps become sRGB BC7, normal maps two-channel BC5 and packed data maps (metallic-roughness,
occlusion) linear BC7, all zstd-supercompressed. Bevy reads embedded `image/ktx2` buffers directly
(the `ktx2` and `zstd_rust` features), so the game skips PNG/JPEG decoding and CPU mip generation,
and the textures take a quarter of the VRAM and bandwidth.

Run it as the last step of an asset's pipeline: Blender and glTF Transform cannot read the converted
textures. Images that are already KTX2 are left alone, so re-running is safe.

    python3 tools/compress_glb_textures.py assets/models/base/hangar.glb [more.glb ...]

Requires the ctt CLI (the encoder behind Bevy's CompressedImageSaver): cargo install ctt-cli
"""

import argparse
import json
import struct
import subprocess
import tempfile
from pathlib import Path

GLB_MAGIC, JSON_CHUNK, BIN_CHUNK = 0x46546C67, 0x4E4F534A, 0x004E4942
COLOR_SLOTS = {"baseColorTexture", "emissiveTexture"}


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
    return [
        "-f", "bc7", "--ic", "linear", "--mipmap", "--mipmap-filter", "lanczos3",
        "--bc7e-opts", "perceptual=false",
    ]


def compress(image_bytes, mime_type, role, quality, scratch):
    source = scratch / ("source.png" if mime_type == "image/png" else "source.jpg")
    target = scratch / "target.ktx2"
    source.write_bytes(image_bytes)
    subprocess.run(
        ["ctt", str(source), "-o", str(target), "--quality", quality, "--zstd=19"]
        + ctt_arguments(role),
        check=True,
        capture_output=True,
    )
    return target.read_bytes()


def convert(path, quality):
    gltf, binary = read_glb(path)
    roles = image_roles(gltf)
    replaced = {}
    with tempfile.TemporaryDirectory() as scratch:
        for index, image in enumerate(gltf.get("images", [])):
            if image.get("mimeType") == "image/ktx2" or index not in roles:
                continue
            if len(roles[index]) > 1:
                raise ValueError(f"{path}: image {index} is shared by {sorted(roles[index])} slots")
            view = gltf["bufferViews"][image["bufferView"]]
            start = view.get("byteOffset", 0)
            role = next(iter(roles[index]))
            replaced[image["bufferView"]] = compress(
                binary[start : start + view["byteLength"]],
                image.get("mimeType"),
                role,
                quality,
                Path(scratch),
            )
            image["mimeType"] = "image/ktx2"
    if not replaced:
        return 0, 0

    # Repack every view so the new image sizes keep the others 8-byte aligned.
    packed = bytearray()
    for index, view in enumerate(gltf["bufferViews"]):
        start = view.get("byteOffset", 0)
        chunk = replaced.get(index, binary[start : start + view["byteLength"]])
        packed += b"\0" * (-len(packed) % 8)
        view["byteOffset"] = len(packed)
        view["byteLength"] = len(chunk)
        packed += chunk
    gltf["buffers"][0]["byteLength"] = len(packed)
    before = path.stat().st_size
    write_glb(path, gltf, bytes(packed))
    return before, path.stat().st_size


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("glb", nargs="+", type=Path)
    parser.add_argument("--quality", default="basic", help="ctt quality preset (default: basic)")
    args = parser.parse_args()
    for path in args.glb:
        before, after = convert(path, args.quality)
        if before:
            print(f"{path}: {before / 1e6:.1f} MB -> {after / 1e6:.1f} MB")
        else:
            print(f"{path}: already compressed")


if __name__ == "__main__":
    main()
