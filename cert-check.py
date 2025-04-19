import os
import OpenSSL.crypto
from datetime import datetime

def check_certificate(cert_path):
    """Check certificate details and validity"""
    try:
        with open(cert_path, 'r') as f:
            cert_data = f.read()
        
        # Parse the certificate
        cert = OpenSSL.crypto.load_certificate(
            OpenSSL.crypto.FILETYPE_PEM, 
            cert_data
        )
        
        # Extract basic info
        subject = cert.get_subject()
        issuer = cert.get_issuer()
        
        # Format dates
        not_before = datetime.strptime(cert.get_notBefore().decode('ascii'), '%Y%m%d%H%M%SZ')
        not_after = datetime.strptime(cert.get_notAfter().decode('ascii'), '%Y%m%d%H%M%SZ')
        now = datetime.now()
        
        # Print certificate info
        print(f"Certificate: {cert_path}")
        print(f"Subject: {subject.CN}")
        print(f"Issuer: {issuer.CN}")
        print(f"Valid from: {not_before}")
        print(f"Valid until: {not_after}")
        
        # Check validity
        if now < not_before:
            print("Status: Not yet valid")
        elif now > not_after:
            print("Status: EXPIRED")
        else:
            print("Status: Valid")
        
        return True
    except Exception as e:
        print(f"Error checking {cert_path}: {e}")
        return False

def check_private_key(key_path, cert_path=None):
    """Check private key and optionally verify it matches a certificate"""
    try:
        with open(key_path, 'r') as f:
            key_data = f.read()
        
        # Parse the key
        key = OpenSSL.crypto.load_privatekey(
            OpenSSL.crypto.FILETYPE_PEM, 
            key_data
        )
        
        print(f"Private Key: {key_path}")
        print(f"Key type: {key.type()}")
        print(f"Bits: {key.bits()}")
        
        # Verify the key matches the certificate if provided
        if cert_path:
            with open(cert_path, 'r') as f:
                cert_data = f.read()
            cert = OpenSSL.crypto.load_certificate(
                OpenSSL.crypto.FILETYPE_PEM, 
                cert_data
            )
            
            # Check context
            context = OpenSSL.SSL.Context(OpenSSL.SSL.TLSv1_2_METHOD)
            context.use_privatekey(key)
            context.use_certificate(cert)
            try:
                context.check_privatekey()
                print("Key matches certificate ✅")
            except OpenSSL.SSL.Error:
                print("Key does NOT match certificate ❌")
        
        return True
    except Exception as e:
        print(f"Error checking {key_path}: {e}")
        return False

def main():
    aws_dir = "aws"
    
    # Check all certificates and keys in aws directory
    if os.path.exists(aws_dir):
        print(f"Checking files in {aws_dir}...")
        files = os.listdir(aws_dir)
        
        # Check CA certificates first
        ca_certs = [f for f in files if "root" in f.lower() or "ca" in f.lower()]
        for ca in ca_certs:
            print("\n" + "="*50)
            check_certificate(os.path.join(aws_dir, ca))
        
        # Check client certificates
        client_certs = [f for f in files if f.endswith(".pem") or f.endswith(".crt") or f.endswith(".cert")]
        client_certs = [f for f in client_certs if f not in ca_certs]
        
        for cert in client_certs:
            print("\n" + "="*50)
            check_certificate(os.path.join(aws_dir, cert))
        
        # Check private keys and validate against certificates
        private_keys = [f for f in files if "key" in f.lower() or "private" in f.lower()]
        for key in private_keys:
            print("\n" + "="*50)
            # Try to find a matching certificate
            key_base = key.split('.')[0]
            matching_certs = [c for c in client_certs if key_base in c]
            
            if matching_certs:
                check_private_key(os.path.join(aws_dir, key), 
                                  os.path.join(aws_dir, matching_certs[0]))
            else:
                check_private_key(os.path.join(aws_dir, key))
    else:
        print(f"Directory {aws_dir} not found")

if __name__ == "__main__":
    main()