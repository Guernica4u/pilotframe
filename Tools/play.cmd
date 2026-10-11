@echo off
rem Build and start Pilotframe. The game reads Resources\ and its assets from
rem the project folder, so it runs from there. A desktop shortcut can point here.
cd /d "%~dp0.."
set "BEVY_ASSET_ROOT=%CD%\crates\pf-game"
echo Building Pilotframe...
cargo build -p pf-game --release
if errorlevel 1 (
    echo.
    echo The build failed, see above.
    pause
    exit /b 1
)
start "" "%CD%\target\release\pf-game.exe"
