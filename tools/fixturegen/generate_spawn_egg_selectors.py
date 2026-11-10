#!/usr/bin/env python3
"""Derive legacy egg selectors from pinned actor identifiers, read in place."""
import argparse
import hashlib
import json
import struct
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SOURCE_COMMIT = '7e91146842837b2267a8e4d8bf8b8dab5e482970'
SOURCE_BLOB = '8a6710ade1f60a85391bbb383e979a0bd0cdcf50'
OUTPUT = ROOT / 'crates/pack-compiler/entity/data/spawn-egg-selectors-v2225.json'


class NbtReader:
    """Read the network NBT scalar/compound/list types in the pinned actor list."""
    def __init__(self, data):
        self.data = data
        self.offset = 0

    def take(self, count):
        if count < 0 or self.offset + count > len(self.data):
            raise ValueError('truncated actor identifiers')
        result = self.data[self.offset:self.offset + count]
        self.offset += count
        return result

    def unsigned(self):
        result = 0
        for shift in range(0, 70, 7):
            value = self.take(1)[0]
            result |= (value & 127) << shift
            if not value & 128:
                return result
        raise ValueError('oversized actor identifier integer')

    def integer(self):
        value = self.unsigned()
        return (value >> 1) ^ -(value & 1)

    def string(self):
        return self.take(self.unsigned()).decode('utf-8')

    def payload(self, tag):
        if tag == 1:
            return struct.unpack('b', self.take(1))[0]
        if tag == 3:
            return self.integer()
        if tag == 8:
            return self.string()
        if tag == 9:
            child = self.take(1)[0]
            return [self.payload(child) for _ in range(self.integer())]
        if tag == 10:
            result = {}
            while (child := self.take(1)[0]):
                name = self.string()
                if name in result:
                    raise ValueError('duplicate actor identifier field')
                result[name] = self.payload(child)
            return result
        raise ValueError(f'unexpected actor identifier NBT tag: {tag}')


def generated(source):
    """Select spawnable actors and compatibility aliases from checked source facts."""
    blob = hashlib.sha1(b'blob ' + str(len(source)).encode() + b'\0' + source).hexdigest()
    if blob != SOURCE_BLOB:
        raise ValueError('actor identifiers differ from the pinned source blob')
    reader = NbtReader(source)
    if reader.take(1) != b'\x0a' or reader.string():
        raise ValueError('expected an unnamed actor identifier compound')
    actors = reader.payload(10)['idlist']
    if reader.offset != len(source):
        raise ValueError('trailing actor identifiers')
    policy = json.loads((ROOT / 'crates/asset-compiler/data/registry-policy.json').read_text())
    replacements = policy['pack_rules']['superseded_eggs']
    selectors = sorted([[actor['rid'], actor['id']] for actor in actors
                        if actor['hasspawnegg'] or actor['id'] in replacements])
    if len({row[0] for row in selectors}) != len(selectors):
        raise ValueError('ambiguous egg selector')
    target = json.loads((ROOT / 'assets/bedrock-target.json').read_text())
    return json.dumps({
        'wire_protocol': target['wire_protocol'],
        'source': {'repository': 'pmmp/BedrockData', 'commit': SOURCE_COMMIT,
                   'path': 'entity_identifiers.nbt', 'git_blob': blob,
                   'sha256': hashlib.sha256(source).hexdigest()},
        'selectors': selectors,
    }, indent=2) + '\n'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path, help='existing entity_identifiers.nbt; never copied')
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    text = generated(args.source.read_bytes())
    if args.check:
        if OUTPUT.read_text() != text:
            raise SystemExit('spawn egg selector facts need regeneration')
        print('Spawn egg selector facts match the pinned actor identifiers.')
    else:
        OUTPUT.write_text(text)
        print(f'Generated {OUTPUT.relative_to(ROOT)}')


if __name__ == '__main__':
    main()
