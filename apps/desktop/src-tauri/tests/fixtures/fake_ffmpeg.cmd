@echo off
set "last="
:next
if "%~1"=="" goto write
set "last=%~1"
shift
goto next
:write
echo RIFF fake normalized wave>%last%
exit /b 0
