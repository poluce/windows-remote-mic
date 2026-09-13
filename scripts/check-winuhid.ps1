Write-Output "==== files ===="
Get-ChildItem "$env:LOCALAPPDATA\RemoteMic\WinUHid" -Recurse -ErrorAction SilentlyContinue | Select-Object FullName, Length
Write-Output "==== pnputil WinUHid ===="
pnputil /enum-drivers | Select-String -Pattern "WinUHid" -Context 0,6
Write-Output "==== devices ===="
Get-PnpDevice -ErrorAction SilentlyContinue | Where-Object { $_.InstanceId -match "WinUHid" -or $_.FriendlyName -match "WinUHid" } | Format-Table Status, Class, FriendlyName, InstanceId -AutoSize
Write-Output "==== admin? ===="
$p = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
Write-Output $p.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
