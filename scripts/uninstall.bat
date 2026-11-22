@echo off
rem Removes what Long Range keeps outside its own folder: settings, logs, replays, your presets and
rem downloaded maps (%APPDATA%\LongRange). Then tells you to delete the game folder itself.
setlocal
set "DIR=%APPDATA%\LongRange"
if defined LR_CONFIG set "DIR=%LR_CONFIG%"

if not exist "%DIR%" goto none
echo This deletes your Long Range settings, replays, presets and downloaded maps:
echo   %DIR%
set "ANSWER="
set /p ANSWER=Delete it? [y/N] 
if /i "%ANSWER%"=="y" goto delete
echo Kept %DIR%
goto game

:delete
rmdir /s /q "%DIR%"
echo Deleted %DIR%
goto game

:none
echo No settings folder at %DIR% - nothing to remove.

:game
echo.
echo The game itself is in this folder; delete it to finish uninstalling:
echo   %~dp0
echo.
pause
