"""Decode each authored mip. Keep source orientation and never resize a level."""
import struct
from UnityPy.export.Texture2DConverter import parse_image_data
import texture2ddecoder


def decoded_mips(obj):
    raw=obj.get_image_data()
    kind=int(obj.m_TextureFormat)
    width,height=obj.m_Width,obj.m_Height
    count=obj.m_MipCount
    if count<1 or count>max(width,height).bit_length():raise ValueError('Invalid source mip count')
    payload=bytearray(struct.pack('<4sIII',b'SMIP',width,height,count))
    offset=0
    for level in range(count):
        w,h=max(1,width>>level),max(1,height>>level)
        if kind in (28,29):
            data=texture2ddecoder.unpack_unity_crunch(raw,level)
            format_id=10 if kind==28 else 12
            expected=max(1,(w+3)//4)*max(1,(h+3)//4)*(8 if kind==28 else 16)
        elif kind in (1,3,4):
            expected=w*h*{1:1,3:3,4:4}[kind]
            data=raw[offset:offset+expected];offset+=expected;format_id=kind
        else:raise ValueError(f'Unsupported required source mip format {kind}')
        if len(data)!=expected:raise ValueError(f'Source mip {level} byte count')
        image=parse_image_data(data,w,h,format_id,(2021,3,18,1),0,flip=False).convert('RGBA')
        payload.extend(image.tobytes())
    if kind in (1,3,4) and offset!=len(raw):raise ValueError("Source mip stream byte count")
    return bytes(payload)


def validate_mips(payload,width,height,count):
    if len(payload)<16 or struct.unpack('<4sIII',payload[:16])!=(b'SMIP',width,height,count):
        raise ValueError('Decoded source mip header')
    expected=16+sum(max(1,width>>i)*max(1,height>>i)*4 for i in range(count))
    if len(payload)!=expected:raise ValueError('Decoded source mip byte count')
