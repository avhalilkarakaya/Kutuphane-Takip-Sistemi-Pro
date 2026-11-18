@echo off
setlocal
cd /d "%~dp0"
title Kutuphane Takip Sistemi Pro - Derleme

rem Rust yeni kurulduysa PATH henuz guncellenmemis olabilir
if exist "%USERPROFILE%\.cargo\bin\cargo.exe" set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"

where cargo >nul 2>nul
if errorlevel 1 goto norust

echo.
echo === Kutuphane Takip Sistemi Pro derleniyor ===
echo Ilk derleme internetten paketleri indirdigi icin 5-10 dakika surebilir.
echo.

if /i "%~1"=="32" goto x86

cargo build --release
if errorlevel 1 goto fail
set "SRC=target\release\Kutuphane_Takip_Sistemi_Pro.exe"
set "DST=dist\Kutuphane_Takip_Sistemi_Pro.exe"
goto copy

:x86
rustup target add i686-pc-windows-msvc
cargo build --release --target i686-pc-windows-msvc
if errorlevel 1 goto fail
set "SRC=target\i686-pc-windows-msvc\release\Kutuphane_Takip_Sistemi_Pro.exe"
set "DST=dist\Kutuphane_Takip_Sistemi_Pro_32bit.exe"

:copy
if not exist dist mkdir dist
copy /y "%SRC%" "%DST%" >nul
echo.
echo TAMAMLANDI: %DST%
echo Bu exe dosyasini eski uygulamanin bulundugu klasore koyarsaniz
echo mevcut verileriniz aynen kullanilir.
echo.
pause
exit /b 0

:norust
echo.
echo [!] Bu bilgisayarda Rust bulunamadi.
echo.
echo     1. https://rustup.rs adresinden rustup-init.exe dosyasini indirip calistirin.
echo     2. Kurulum "Visual Studio C++ Build Tools" isterse kurulmasini onaylayin.
echo     3. Kurulum bitince bu pencereyi kapatip derle.bat dosyasini yeniden calistirin.
echo.
pause
exit /b 1

:fail
echo.
echo [!] Derleme basarisiz oldu. Yukaridaki hata mesajini kontrol edin.
echo     "link.exe not found" yaziyorsa Visual Studio C++ Build Tools kurulu degildir.
echo.
pause
exit /b 1
