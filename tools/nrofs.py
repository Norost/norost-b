#!/usr/bin/env python3

from argparse import ArgumentParser
from struct import pack, unpack
from os import path
from pathlib import Path
import os

MAGIC = b"NrRdOnly"
VERSION = 0

def write_header(f, block_size, file_count):
    f.write(pack("<8sBBxxI", MAGIC, VERSION, block_size, file_count))

def write_entry(f, filename_addr, block_addr, file_size):
    f.write(pack("<III", filename_addr, block_addr, file_size))

def write_string(f, s):
    s = s.encode('utf-8')
    if len(s) > 255:
        raise StringTooLargeException()
    f.write(bytes([len(s)]))
    f.write(s)

def read_header(f) -> (int, int):
    (magic, version, block_size, file_count) = unpack("<8sBBxxI", f.read(16))
    if magic != MAGIC:
        raise BadMagicException()
    if version != VERSION:
        raise UnsupportedVersionException()
    return (block_size, file_count)

def read_entry(f) -> (int, int, int):
    return unpack("<III", f.read(12))

def read_string(f) -> str:
    l = f.read(1)[0]
    return f.read(l).decode('utf-8')

class StringTooLargeException(Exception):
    pass

class BadMagicException(Exception):
    pass

class UnsupportedVersionException(Exception):
    pass

def create(args):
    assert args.files != []

    if args.change_dir is not None:
        os.chdir(args.change_dir)

    # Ensure we don't have any duplicates
    files = set(map(Path, args.files))
    if args.recursive:
        # Collect all files and discard directory entries
        all_files = set()
        for f in files:
            if path.isdir(f):
                #all_files |= {e for e in f.glob('**') if path.isfile(e)}
                all_files |= {*filter(lambda e: e.is_file(), f.glob('**/*'))}
            else:
                all_files.add(f)
        files = all_files
    # I have a strong preference for sorted files
    files = sorted(map(str, files))
    file_count = len(files)

    # Open file twice so we can write entries & data simultaneously without explicitly
    # seeking all the time
    ar_meta = open(args.output, 'wb')
    ar_data = open(args.output, 'wb')

    write_header(ar_meta, args.block_size, file_count)

    block_mask = (1 << args.block_size) - 1
    calc_blocks = lambda n: (n + block_mask) >> args.block_size

    total_strings_size = sum(map(len, files))
    total_meta_size = 16 + 12 * file_count + total_strings_size
    total_meta_blocks = calc_blocks(total_meta_size)

    # Write entries & file data
    ar_data.seek(total_meta_blocks << args.block_size)
    next_string_addr = total_meta_size - total_strings_size
    next_block_addr = total_meta_blocks
    for f in files:
        if args.verbose:
            print(f)
        s = path.getsize(f)
        write_entry(ar_meta, next_string_addr, next_block_addr, s)
        bs = calc_blocks(s)
        with open(f, 'rb') as f_data:
            while True:
                l = f_data.read(1 << 16)
                if len(l) == 0:
                    break
                ar_data.write(l)
        next_block_addr += bs
        if s & block_mask != 0:
            ar_data.seek(next_block_addr << args.block_size)
        next_string_addr += 1 + len(f)

    # Write strings
    for f in files:
        write_string(ar_meta, f)

def list_files(args):
    ar_meta = open(args.output, 'rb')
    ar_data = open(args.output, 'rb')
    block_size, file_count = read_header(ar_meta)
    if args.verbose:
        print('block size:', block_size)
        print('file count:', file_count)
    for _ in range(file_count):
        filename_addr, block_addr, file_size = read_entry(ar_meta)
        ar_data.seek(filename_addr)
        if args.verbose:
            print('%8d  %8d  %s' % (block_addr, file_size, read_string(ar_data)))
        else:
            print('%8d  %s' % (file_size, read_string(ar_data)))

if __name__ == '__main__':
    p = ArgumentParser()
    p.add_argument('output')
    p.add_argument('files', nargs='*')
    p.add_argument('-l', '--list', action='store_true', help='List all files')
    p.add_argument('-d', '--extract', action='store_true', help='Extract one or more files')
    p.add_argument('-r', '--recursive', action='store_true')
    p.add_argument('-v', '--verbose', action='store_true')
    p.add_argument('-b', '--block-size', type=int, default=12, help="Block size as a power of 2")
    p.add_argument('-C', '--change-dir', help='Change to the given directory before collecting files')
    args = p.parse_args()

    if args.list:
        list_files(args)
    elif args.extract:
        assert 0, "todo"
    else:
        create(args)
