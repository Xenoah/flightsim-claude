#!/usr/bin/env python3
"""Emit an isolated -X-nose candidate from exact original Meadow geometry.

MIT OR Apache-2.0, matching the original source and project. Standard library only.
This is deliberately an adapter for one pinned asset, not a general glTF editor.
It never opens the unresolved Light Single GLB or writes into a repository.
"""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import struct

HERE = Path(__file__).resolve().parent
SOURCE_SHA256 = 'c873b59a5e638c16bd2fb36c789704a41b1146ea90c7c44950257cbf5f806412'


def adapt(raw):
    if hashlib.sha256(raw).hexdigest() != SOURCE_SHA256:
        raise ValueError('Expected exact, reviewed original Meadow Trainer source GLB')
    json_size, json_kind = struct.unpack_from('<II', raw, 12)
    binary_size, binary_kind = struct.unpack_from('<II', raw, 20 + json_size)
    assert struct.unpack_from('<4sII', raw) == (b'glTF', 2, len(raw))
    assert (json_kind, binary_kind) == (0x4e4f534a, 0x004e4942)
    assert 28 + json_size + binary_size == len(raw)
    document = json.loads(raw[20:20 + json_size])
    binary = bytearray(raw[28 + json_size:])
    original_document = copy.deepcopy(document)
    changed = set()
    occupied = set()
    for mesh in document['meshes']:
        for primitive in mesh['primitives']:
            assert set(primitive['attributes']) <= {'POSITION', 'NORMAL', 'TEXCOORD_0'}
            assert 'targets' not in primitive
            for name in ('POSITION', 'NORMAL'):
                index = primitive['attributes'][name]
                if index in changed:
                    continue
                accessor = document['accessors'][index]
                assert accessor['type'] == 'VEC3' and accessor['componentType'] == 5126
                assert not accessor.get('sparse') and not accessor.get('normalized')
                view = document['bufferViews'][accessor['bufferView']]
                assert view['buffer'] == 0 and view.get('byteStride', 12) == 12
                assert accessor.get('byteOffset', 0) == 0
                assert view['byteLength'] == accessor['count'] * 12
                start = view.get('byteOffset', 0)
                offsets = set(range(start, start + view['byteLength']))
                assert not occupied.intersection(offsets), 'Unexpected overlapping accessor data'
                occupied.update(offsets)
                points = []
                for offset in range(start, start + view['byteLength'], 12):
                    x, y, z = struct.unpack_from('<3f', binary, offset)
                    # Exact right-handed -90 degrees around glTF +Y; determinant +1.
                    # Permutation/sign only: no trigonometric rounding, scaling,
                    # origin recentering, reflection, or triangle reordering.
                    rotated = (-z, y, x)
                    struct.pack_into('<3f', binary, offset, *rotated)
                    points.append(rotated)
                for bound, operation in [('min', min), ('max', max)]:
                    if bound in accessor:
                        accessor[bound] = [operation(p[i] for p in points) for i in range(3)]
                changed.add(index)
    for node in document['nodes']:
        assert set(node) <= {'name', 'mesh', 'extras'}, 'Expected identity nodes only'
    assert document['nodes'] == original_document['nodes']
    json_bytes = json.dumps(document, separators=(',', ':')).encode('utf-8')
    json_bytes += b' ' * (-len(json_bytes) % 4)
    assert len(binary) % 4 == 0
    length = 28 + len(json_bytes) + len(binary)
    return (struct.pack('<4sII', b'glTF', 2, length)
            + struct.pack('<II', len(json_bytes), 0x4e4f534a) + json_bytes
            + struct.pack('<II', len(binary), 0x004e4942) + binary)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=HERE / 'assets/aircraft/light_single.glb')
    args = parser.parse_args()
    destination = args.output.resolve()
    # Keep preparation reversible, even when accidentally passed a production path.
    if not destination.is_relative_to(HERE) or destination.is_relative_to(HERE / 'source'):
        raise ValueError('Output must remain in the isolated candidate directory, outside source/')
    raw = adapt((HERE / 'source/meadow_trainer.glb').read_bytes())
    destination.parent.mkdir(parents=True, exist_ok=True)
    if destination.exists() and destination.read_bytes() != raw:
        raise ValueError('Refusing to overwrite a different existing candidate')
    destination.write_bytes(raw)
    print(json.dumps({'path': str(destination.relative_to(HERE)), 'bytes': len(raw),
                      'sha256': hashlib.sha256(raw).hexdigest(), 'source_sha256': SOURCE_SHA256}, indent=2))


if __name__ == '__main__':
    main()
