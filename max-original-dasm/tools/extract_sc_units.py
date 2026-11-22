import struct, json, sys
path=r"D:\dev\max-port-debug\assets\PATCHES.RES"
data=open(path,"rb").read()
sig,toff,tsize=struct.unpack_from("<4sII",data,0)
assert sig==b"RES0",sig
n=tsize//16
for i in range(n):
    off=toff+i*16
    tag=data[off:off+8].split(b"\x00")[0].decode("latin1")
    doff,dsize=struct.unpack_from("<II",data,off+8)
    if tag=="SC_UNITS":
        raw=data[doff:doff+dsize]
        tbl=bytes([0x6c,0x57,0x36,0xe6,0x81,0xe0,0x72,0x8a,0xd3,0xd9,0xff,0x54,0x48,0x3a,0xcd,0x75,0xd5,0x0d,0xe9,0xe7,0x7a,0x57,0xae,0xeb,0x61,0x16,0xb0,0x35,0x55,0x62,0xed,0xd1])
        dec=bytes(b^tbl[j%32] for j,b in enumerate(raw))
        open(r"D:\dev\max-port-debug\build\SC_UNITS.json","wb").write(dec)
        print("wrote",dsize)
        break
