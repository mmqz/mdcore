# -*- coding: utf-8 -*-
"""生成真实结构的签名 PDF 夹具（issue #116 任务 2 验签面）。

用 pyhanko：真 PKCS#7/CMS SignedData + 自签 X.509 证书。
结构完全真实（Adobe 也认），只是证书非 CA 签发——
本机无真实 CA 签名样本，如实登记（Lead 任务书允许此路径）。
"""
import sys, os, datetime
sys.stdout.reconfigure(encoding="utf-8")

from cryptography import x509 as cx509
from cryptography.x509.oid import NameOID
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import rsa

OUT = os.path.dirname(os.path.abspath(__file__))
key_path = os.path.join(OUT, '_signer.key')
crt_path = os.path.join(OUT, '_signer.crt')

key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
subject = issuer = cx509.Name([
    cx509.NameAttribute(NameOID.COUNTRY_NAME, 'CN'),
    cx509.NameAttribute(NameOID.ORGANIZATION_NAME, '某建设工程有限公司'),
    cx509.NameAttribute(NameOID.COMMON_NAME, '投标人签字人'),
])
now = datetime.datetime(2026, 1, 1)
cert = (cx509.CertificateBuilder()
    .subject_name(subject).issuer_name(issuer)
    .public_key(key.public_key())
    .serial_number(0x1234567890ABCDEF)
    .not_valid_before(now)
    .not_valid_after(now + datetime.timedelta(days=3650))
    .sign(key, hashes.SHA256()))

with open(key_path, 'wb') as f:
    f.write(key.private_bytes(serialization.Encoding.PEM,
        serialization.PrivateFormat.PKCS8, serialization.NoEncryption()))
with open(crt_path, 'wb') as f:
    f.write(cert.public_bytes(serialization.Encoding.PEM))

from pyhanko.sign import signers
from pyhanko.sign.fields import SigSeedSubFilter
from pyhanko.pdf_utils.incremental_writer import IncrementalPdfFileWriter

signer = signers.SimpleSigner.load(key_path, crt_path)
meta = signers.PdfSignatureMetadata(
    field_name='Signature1',
    reason='投标文件签署',
    location='北京市',
    subfilter=SigSeedSubFilter.PADES,
)
base = os.path.join(OUT, 'form_bid_letter.pdf')
out = os.path.join(OUT, 'signed_sample.pdf')
with open(base, 'rb') as f:
    w = IncrementalPdfFileWriter(f)
    with open(out, 'wb') as o:
        signers.sign_pdf(w, meta, signer=signer, output=o)

os.remove(key_path); os.remove(crt_path)
print('wrote', out, os.path.getsize(out), 'bytes')