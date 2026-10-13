param([switch]$CreateCertificate)

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$exePath = Join-Path $projectRoot 'AudioRouter.exe'
$certificatePath = Join-Path $projectRoot 'AudioRouter-cantonadong.cer'
$friendlyName = 'AudioRouter self-signed code signing'
$email = 'cantonadong@gmail.com'

if (-not (Test-Path -LiteralPath $exePath -PathType Leaf)) {
    throw "Executable not found: $exePath"
}

$certificate = Get-ChildItem Cert:\CurrentUser\My -CodeSigningCert |
    Where-Object { $_.GetNameInfo([System.Security.Cryptography.X509Certificates.X509NameType]::SimpleName, $false) -eq 'cantonadong' -and $_.GetNameInfo([System.Security.Cryptography.X509Certificates.X509NameType]::EmailName, $false) -eq $email -and $_.FriendlyName -eq $friendlyName -and $_.HasPrivateKey -and $_.NotAfter -gt (Get-Date).AddDays(7) } |
    Sort-Object NotAfter -Descending |
    Select-Object -First 1

if (-not $certificate) {
    if (-not $CreateCertificate) {
        throw 'Signing certificate is unavailable. Run tools/sign.ps1 -CreateCertificate under the normal Windows user.'
    }
    $certificate = New-SelfSignedCertificate -Type CodeSigningCert -Subject "CN=cantonadong, E=$email" `
        -FriendlyName $friendlyName -CertStoreLocation Cert:\CurrentUser\My `
        -KeyAlgorithm RSA -KeyLength 3072 -HashAlgorithm SHA256 -KeyExportPolicy NonExportable `
        -NotAfter (Get-Date).AddYears(3)
}

Get-Process -Name AudioRouter -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction Stop
$null = Export-Certificate -Cert $certificate -FilePath $certificatePath -Type CERT -Force
$packagingTime = Get-Date
$null = Set-AuthenticodeSignature -LiteralPath $exePath -Certificate $certificate -HashAlgorithm SHA256 -TimestampServer 'http://timestamp.digicert.com'
$signature = Get-AuthenticodeSignature -LiteralPath $exePath
if (-not $signature.SignerCertificate -or $signature.SignerCertificate.Thumbprint -ne $certificate.Thumbprint) {
    throw "Signing failed: $($signature.StatusMessage)"
}
if ($signature.Status -eq 'HashMismatch' -or $signature.Status -eq 'NotSigned') {
    throw "Signature verification failed: $($signature.StatusMessage)"
}
if (-not $signature.TimeStamperCertificate) {
    throw 'The EXE was signed, but the timestamp server did not return a timestamp. Packaging is incomplete.'
}
# Read and verify the embedded signed timestamp, rather than just the file date.
$null = Add-Type -AssemblyName System.Security
$exeBytes = [System.IO.File]::ReadAllBytes($exePath)
$peOffset = [BitConverter]::ToInt32($exeBytes, 60)
$optionalHeader = $peOffset + 24
$directoryOffset = if ([BitConverter]::ToUInt16($exeBytes, $optionalHeader) -eq 0x20b) { 112 } else { 96 }
$securityDirectory = $optionalHeader + $directoryOffset + 32
$signatureOffset = [BitConverter]::ToInt32($exeBytes, $securityDirectory)
$signatureLength = [BitConverter]::ToInt32($exeBytes, $signatureOffset) - 8
$cmsBytes = New-Object byte[] $signatureLength
[Array]::Copy($exeBytes, $signatureOffset + 8, $cmsBytes, 0, $signatureLength)
$cms = New-Object System.Security.Cryptography.Pkcs.SignedCms
$cms.Decode($cmsBytes)
$cms.CheckSignature($true)
$signedTime = $null
foreach ($signer in $cms.SignerInfos) {
    foreach ($counterSigner in $signer.CounterSignerInfos) {
        foreach ($attribute in $counterSigner.SignedAttributes) {
            if ($attribute.Oid.Value -eq '1.2.840.113549.1.9.5') {
                $timeAttribute = New-Object System.Security.Cryptography.Pkcs.Pkcs9SigningTime -ArgumentList (,$attribute.Values[0].RawData)
                $signedTime = $timeAttribute.SigningTime.ToLocalTime()
            }
        }
    }
}
if (-not $signedTime) { throw 'Embedded signing time is missing.' }
$signature | Select-Object Path, Status, StatusMessage, SignatureType,
    @{Name='Signer';Expression={$_.SignerCertificate.Subject}},
    @{Name='Email';Expression={$_.SignerCertificate.GetNameInfo([System.Security.Cryptography.X509Certificates.X509NameType]::EmailName, $false)}},
    @{Name='Thumbprint';Expression={$_.SignerCertificate.Thumbprint}},
    @{Name='Expires';Expression={$_.SignerCertificate.NotAfter}}
Write-Output "Public certificate: $certificatePath"
Write-Output "Packaging/signing started: $($packagingTime.ToString('o'))"
Write-Output "Timestamp authority: $($signature.TimeStamperCertificate.Subject)"
Write-Output "Embedded signing time: $($signedTime.ToString('o'))"
