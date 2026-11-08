$ErrorActionPreference = "Stop"

$ProjectRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$AndroidDir  = $PSScriptRoot
$BuildDir    = Join-Path $AndroidDir "build"
$DistDir     = Join-Path $ProjectRoot "DIST-RELEASES\Android-Cliente"

$SdkRoot     = "$env:LOCALAPPDATA\Android\Sdk"
$BuildTools  = Join-Path $SdkRoot "build-tools\36.0.0"
$PlatformJar = Join-Path $SdkRoot "platforms\android-34\android.jar"

$JavaHome    = $env:JAVA_HOME
if (-not $JavaHome -or -not (Test-Path "$JavaHome\bin\javac.exe")) {
    $JavaHome = "C:\Program Files\Eclipse Adoptium\jdk-21.0.8.9-hotspot"
}

$Aapt2       = Join-Path $BuildTools "aapt2.exe"
$D8          = Join-Path $BuildTools "d8.bat"
$ZipAlign    = Join-Path $BuildTools "zipalign.exe"
$ApkSigner   = Join-Path $BuildTools "apksigner.bat"
$Javac       = Join-Path $JavaHome "bin\javac.exe"
$JarTool     = Join-Path $JavaHome "bin\jar.exe"
$Keytool     = Join-Path $JavaHome "bin\keytool.exe"

Write-Host "[1/6] Preparando directorios de compilacion..." -ForegroundColor Cyan
if (Test-Path $BuildDir) { Remove-Item -Recurse -Force $BuildDir }
New-Item -ItemType Directory -Force -Path "$BuildDir\compiled_res" | Out-Null
New-Item -ItemType Directory -Force -Path "$BuildDir\gen" | Out-Null
New-Item -ItemType Directory -Force -Path "$BuildDir\obj" | Out-Null
New-Item -ItemType Directory -Force -Path "$BuildDir\dex" | Out-Null
New-Item -ItemType Directory -Force -Path $DistDir | Out-Null

Write-Host "[2/6] Compilando recursos XML y Vector Drawables con aapt2..." -ForegroundColor Cyan
& $Aapt2 compile --dir "$AndroidDir\res" -o "$BuildDir\compiled_res\res.zip"
if ($LASTEXITCODE -ne 0) { throw "Fallo en aapt2 compile" }

& $Aapt2 link `
    -I $PlatformJar `
    --manifest "$AndroidDir\AndroidManifest.xml" `
    --java "$BuildDir\gen" `
    -o "$BuildDir\unaligned.apk" `
    "$BuildDir\compiled_res\res.zip"
if ($LASTEXITCODE -ne 0) { throw "Fallo en aapt2 link" }

Write-Host "[3/6] Compilando codigo fuente Java con JDK 21 (bytecode Java 11)..." -ForegroundColor Cyan
$JavaFiles = Get-ChildItem -Path "$AndroidDir\src","$BuildDir\gen" -Filter "*.java" -Recurse | Select-Object -ExpandProperty FullName
& $Javac --release 11 -encoding UTF-8 -cp $PlatformJar -d "$BuildDir\obj" $JavaFiles
if ($LASTEXITCODE -ne 0) { throw "Fallo en javac" }

& $JarTool cf "$BuildDir\classes.jar" -C "$BuildDir\obj" .
if ($LASTEXITCODE -ne 0) { throw "Fallo en jar" }

Write-Host "[4/6] Convirtiendo clases Java a Dalvik Executable (classes.dex) con d8..." -ForegroundColor Cyan
& $D8 --lib $PlatformJar --min-api 24 --output "$BuildDir\dex" "$BuildDir\classes.jar"
if ($LASTEXITCODE -ne 0) { throw "Fallo en d8" }

Write-Host "[5/6] Empaquetando classes.dex y alineando APK con zipalign..." -ForegroundColor Cyan
& $JarTool uf "$BuildDir\unaligned.apk" -C "$BuildDir\dex" classes.dex
if ($LASTEXITCODE -ne 0) { throw "Fallo en jar uf" }

& $ZipAlign -f -p 4 "$BuildDir\unaligned.apk" "$BuildDir\aligned.apk"
if ($LASTEXITCODE -ne 0) { throw "Fallo en zipalign" }

Write-Host "[6/6] Firmando y verificando APK de produccion..." -ForegroundColor Cyan
$KeystorePath = Join-Path $AndroidDir "release-key.jks"
if (-not (Test-Path $KeystorePath)) {
    & $Keytool -genkeypair -v `
        -keystore $KeystorePath `
        -alias adbhub `
        -keyalg RSA `
        -keysize 2048 `
        -validity 10000 `
        -storepass adbhub2026 `
        -keypass adbhub2026 `
        -dname "CN=ADBHUB-LINK, OU=GSM Lab, O=ADBHUB, L=Global, S=Global, C=US"
}

$FinalApk = Join-Path $DistDir "adbhub-client.apk"
& $ApkSigner sign `
    --ks $KeystorePath `
    --ks-key-alias adbhub `
    --ks-pass pass:adbhub2026 `
    --key-pass pass:adbhub2026 `
    --out $FinalApk `
    "$BuildDir\aligned.apk"
if ($LASTEXITCODE -ne 0) { throw "Fallo en apksigner sign" }

& $ApkSigner verify --verbose $FinalApk
if ($LASTEXITCODE -ne 0) { throw "Fallo en apksigner verify" }

Write-Host "`nAPK compilado, alineado y firmado con exito en: $FinalApk" -ForegroundColor Green
