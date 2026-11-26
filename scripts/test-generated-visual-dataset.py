#!/usr/bin/env python3
"""Independently decode ordinary exported fixtures; never read a live sync root."""
import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import struct
import stat
import subprocess
import zlib

class RootEvidenceUnavailable(RuntimeError):
    def __init__(self, code):
        self.hresult=code&0xffffffff
        super().__init__(f'ordinary-data validation refuses managed/unknown root: HRESULT=0x{self.hresult:08x}')


def require_unregistered(code):
    # Generic CFAPI failures never prove that a root is unmanaged.
    if code & 0xffffffff != 0x80070186:
        raise RootEvidenceUnavailable(code)


def require_plain_directory(root):
    # Check original ancestry before any fixture content; never follow a link
    # into another root and mistake that target for our ordinary export.
    for node in (root, *root.parents):
        info=node.lstat()
        if stat.S_ISLNK(info.st_mode) or getattr(info,'st_file_attributes',0)&0x400:
            raise RuntimeError('ordinary-data validation refuses reparse ancestry')
        if not stat.S_ISDIR(info.st_mode):
            raise RuntimeError('ordinary-data validation requires directory ancestry')


def windows_root_preflight(root):
    from ctypes import wintypes
    cf=ctypes.WinDLL('cldapi')
    cf.CfGetSyncRootInfoByPath.argtypes=[wintypes.LPCWSTR,ctypes.c_int,ctypes.c_void_p,wintypes.DWORD,ctypes.c_void_p]
    cf.CfGetSyncRootInfoByPath.restype=ctypes.c_int32
    basic=ctypes.c_int64()
    code=cf.CfGetSyncRootInfoByPath(str(root),0,ctypes.byref(basic),ctypes.sizeof(basic),None)
    filesystem=ctypes.create_unicode_buffer(64)
    kernel=ctypes.WinDLL('kernel32',use_last_error=True)
    kernel.GetVolumeInformationW.argtypes=[wintypes.LPCWSTR,wintypes.LPWSTR,wintypes.DWORD,ctypes.c_void_p,ctypes.c_void_p,ctypes.c_void_p,wintypes.LPWSTR,wintypes.DWORD]
    volume_ok=kernel.GetVolumeInformationW(root.anchor,None,0,None,None,None,filesystem,len(filesystem))
    queries={'path_basic':f'0x{code&0xffffffff:08x}'}
    returned=wintypes.DWORD()
    standard=ctypes.create_string_buffer(8192)
    for label,kind,buffer in [('path_basic_returned',0,basic),('path_standard',1,standard)]:
        result=cf.CfGetSyncRootInfoByPath(str(root),kind,ctypes.byref(buffer),ctypes.sizeof(buffer),ctypes.byref(returned))
        queries[label]=f'0x{result&0xffffffff:08x}'
    kernel.CreateFileW.argtypes=[wintypes.LPCWSTR,wintypes.DWORD,wintypes.DWORD,ctypes.c_void_p,wintypes.DWORD,wintypes.DWORD,wintypes.HANDLE]
    kernel.CreateFileW.restype=wintypes.HANDLE
    kernel.CloseHandle.argtypes=[wintypes.HANDLE]
    # Attributes only, no recall, no reparse traversal, deny DELETE sharing.
    handle=kernel.CreateFileW(str(root),0x80,3,None,3,0x02000000|0x00200000|0x00100000,None)
    if handle==ctypes.c_void_p(-1).value:
        queries['handle_open_error']=ctypes.get_last_error()
    else:
        try:
            cf.CfGetSyncRootInfoByHandle.argtypes=[wintypes.HANDLE,ctypes.c_int,ctypes.c_void_p,wintypes.DWORD,ctypes.c_void_p]
            cf.CfGetSyncRootInfoByHandle.restype=ctypes.c_int32
            for label,kind,buffer in [('handle_basic',0,basic),('handle_standard',1,standard)]:
                result=cf.CfGetSyncRootInfoByHandle(handle,kind,ctypes.byref(buffer),ctypes.sizeof(buffer),ctypes.byref(returned))
                queries[label]=f'0x{result&0xffffffff:08x}'
        finally:
            kernel.CloseHandle(handle)
    print(json.dumps({'preflight_api':'CfGetSyncRootInfoByPath/BASIC','hresult':f'0x{code&0xffffffff:08x}','queries':queries,'filesystem':filesystem.value if volume_ok else 'unknown','root_exists':root.is_dir(),'root_reparse':bool(root.lstat().st_file_attributes&0x400)}),flush=True)
    if 'handle_open_error' in queries:
        raise OSError(queries['handle_open_error'],'ordinary directory metadata handle unavailable')
    if any(int(value,16)<0x80000000 for value in queries.values()):
        raise RootEvidenceUnavailable(0)  # A query found a root; never read it.
    require_unregistered(code)


def validate_png(data):
    assert len(data)==65536
    assert data[:8]==b'\x89PNG\r\n\x1a\n'
    offset=8;chunks=[];payload=b''
    while offset<len(data):
        length=struct.unpack('>I',data[offset:offset+4])[0]
        kind=data[offset+4:offset+8];body=data[offset+8:offset+8+length]
        assert len(body)==length
        crc=struct.unpack('>I',data[offset+8+length:offset+12+length])[0]
        assert zlib.crc32(kind+body)&0xffffffff==crc
        chunks.append(kind.decode('ascii'))
        if kind==b'IHDR':assert struct.unpack('>IIBBBBB',body)==(64,64,8,2,0,0,0)
        if kind==b'IDAT':payload+=body
        offset+=12+length
        if kind==b'IEND':break
    assert offset==len(data) and chunks==['IHDR','IDAT','tEXt','IEND']
    pixels=zlib.decompress(payload)
    expected_pixels=bytes(value for y in range(64) for value in ([0]+[v for x in range(64) for v in (x*4,y*4,(x^y)*4)]))
    assert pixels==expected_pixels
    return {'format':'PNG','width':64,'height':64,'crc_and_zlib_and_pixels':'passed','sha256':hashlib.sha256(data).hexdigest()}


def validate(root):
    root=root.absolute()
    require_plain_directory(root)
    if os.name=='nt':
        windows_root_preflight(root)

    manifest=json.loads(root.with_suffix('.dataset.json').read_text(encoding='utf-8'))
    assert manifest['dataset']=='visual_v1' and manifest['registered'] is False
    files=manifest['files'];folders=manifest['directories']
    assert len(files)==3 and len(folders)==1
    expected=set()
    for entry in folders+files:
        relative=PurePosixPath(entry['relative_path'])
        assert not relative.is_absolute() and len(relative.parts)<=2 and all(p not in ('..','.') for p in relative.parts)
        path=root.joinpath(*relative.parts)
        assert not path.is_symlink() and not getattr(path.lstat(),'st_file_attributes',0)&0x400
        expected.add(path)
    actual=set(root.iterdir())
    actual.update((root/folders[0]['relative_path']).iterdir())
    assert actual==expected, 'unexpected or missing ordinary fixture entry'
    rows=[]
    for entry in files:
        data=(root/entry['relative_path']).read_bytes()
        assert len(data)==entry['size']==65536
        rows.append({'id':entry['id'],'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest()})
        if entry['relative_path'].endswith('.png'):
            rows[-1].update(validate_png(data))
    assert sum(r.get('format')=='PNG' for r in rows)==1
    print(json.dumps({'registered':False,'files':rows,'directory_count':1,'total_file_bytes':196608},ensure_ascii=True))


if __name__=='__main__':
    parser=argparse.ArgumentParser()
    parser.add_argument('directory',type=Path,nargs='?')
    parser.add_argument('--png-from-exporter',type=Path)
    args=parser.parse_args()
    if (args.directory is None)==(args.png_from_exporter is None):
        parser.error('choose one ordinary directory OR a generated-byte exporter')
    if args.png_from_exporter is not None:
        flags=subprocess.CREATE_NO_WINDOW if os.name=='nt' else 0
        result=subprocess.run([str(args.png_from_exporter.resolve(strict=True)),'--png-stdout'],check=True,stdout=subprocess.PIPE,timeout=15,creationflags=flags)
        report=validate_png(result.stdout)
        report.update(source='owned_generator_stdout',filesystem_fixture_read=False,native_acceptance=False)
        print(json.dumps(report))
    else:
        validate(args.directory)
