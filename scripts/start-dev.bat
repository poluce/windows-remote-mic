@echo off
cd /d "F:\B_My_Document\GitHub\windows-remote-mic"
npm run tauri dev > "%TEMP%\remote-mic-dev.log" 2>&1
