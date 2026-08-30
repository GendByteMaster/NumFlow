# Windows driver TestPackage signing

Status: package-preparation procedure only. The development host may build and sign, but it must
not trust, stage, install, load, or bind this driver.

## Certificate lifecycle

`driver/scripts/sign-test-driver.ps1` selects an unexpired code-signing certificate whose exact
subject is `CN=NumFlow Driver Test`, whose private key is available, and which remains valid for at
least 30 days. If none exists, it creates a 3072-bit RSA/SHA-256 certificate under
`Cert:\CurrentUser\My` with a non-exportable private key and a two-year validity period.

The pipeline exports only `NumFlowDriverTest.cer`. It never exports PFX/PVK/PEM/private-key material,
never adds the certificate to a host trust store, and never enables TESTSIGNING. The certificate
thumbprint is printed at the end so the signing identity is auditable.

To rotate deliberately:

```powershell
.\driver\scripts\prepare-test-package.ps1 -ForceNewCertificate
```

Rotation creates a new certificate and does not silently delete old private keys. Remove an old
certificate only as a separate, explicit maintenance action after proving no retained VM package
depends on it.

## Verification semantics

The catalog is signed with SignTool `/fd SHA256`. The pipeline then:

1. verifies the PKCS#7 signature cryptographically;
2. requires exactly one signer matching the exported CER thumbprint;
3. builds a chain against an in-memory custom trust root;
4. runs SignTool `/pa` and reports its host policy result separately;
5. records SHA-256 hashes for SYS, INF, CAT, and CER.

A self-signed test certificate is not trusted by the host, so SignTool `/pa` normally reports an
untrusted root there. This is expected and is not hidden as a PASS. The VM install script imports
the public CER into the disposable VM's Root and TrustedPublisher stores and requires trusted CAT
verification before calling `pnputil`.

## Release boundary

This TestPackage is for the VirtualBox PASS_THROUGH gate only. Production signing, attestation/HLK
requirements, broader device models, timestamp policy, distribution, upgrade, and rollback are not
implemented and must not be inferred from a successful test signature.
