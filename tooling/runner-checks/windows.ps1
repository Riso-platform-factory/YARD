$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
$unsignedCases = @('bundle', 'bundle-secrets', 'install', 'scheme', 'command', 'icon',
    'uninstall-entry', 'shortcut', 'app-user-model-id', 'open', 'screenshot', 'url-cold', 'url-warm',
    'app-mutex', 'user-data-setup', 'uninstall', 'uninstall-keys', 'uninstall-files', 'user-data', 'other-scheme-value')
$signedArtifactCases = @('installer-signature', 'zip-extract', 'zip-signature', 'tampered-signature',
    'install', 'installed-signature', 'uninstaller-signature', 'uninstall')
$northStarCases = @('new-app', 'bundle', 'sample-bundle', 'install', 'sample-install', 'product-name', 'icon',
    'scheme', 'url', 'screenshot', 'coexistence')
$updateCases = @('update-clone', 'update-new-app', 'update', 'update-app-kept', 'update-manifest',
    'bundle', 'install', 'product-name', 'icon', 'scheme', 'url')
$expected = @{
    update = $updateCases
    northstar = $northStarCases
    plain = $unsignedCases + @('coexistence')
    spaced = $unsignedCases
    other = $unsignedCases
    signed = $signedArtifactCases + @('certificate', 'bundle', 'bundle-secrets',
        'signing-cleanup-after-bundle', 'wrong-password', 'wrong-password-secrets', 'trust-cleanup', 'signing-cleanup')
}
$recordFailed = $false
# The host of every URL sent: no extension has it, so the shell refuses each URL.
$urlHost = 'nobody'
$coldUrl = $null
$results = @{}
$clock = [Diagnostics.Stopwatch]::StartNew()
$deadline = if ($env:YARD_CHECK_SECONDS) { [int]$env:YARD_CHECK_SECONDS } else { 10200 }
$checks = Join-Path $PWD 'checks/windows'
New-Item -ItemType Directory -Force $checks | Out-Null
$report = Join-Path $checks 'report.txt'
Set-Content -LiteralPath $report -Value '' -NoNewline
$work = Join-Path $env:RUNNER_TEMP "yard-checks-$([Guid]::NewGuid())"
New-Item -ItemType Directory $work | Out-Null
if (-not $env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR = Join-Path $PWD 'target' }
# Use the installed app's default directories and installer mutex.
Remove-Item Env:YARD_HOME, Env:WINDOWS_CERTIFICATE, Env:WINDOWS_CERTIFICATE_PASSWORD -ErrorAction SilentlyContinue

function Report($status, $case, $message) {
    $message = $message -replace '[\r\n]+', ' '
    $line = "$status $config $($case): $message"
    try {
        Add-Content -LiteralPath $report -Value $line
        $key = "$config $case"
        $results[$key] = 1 + $results[$key]
        if ($status -eq 'FAIL') { $script:recordFailed = $true }
    } catch {
        $script:recordFailed = $true
        Write-Warning 'Could not write a report line.'
    }
    Write-Host $line
}

function Skip($reason, $cases) {
    foreach ($case in $cases) { Report FAIL $case "skipped: $reason" }
}

function Check($case, [scriptblock]$command) {
    if ($clock.Elapsed.TotalSeconds -ge $deadline -and $case -notin @('trust-cleanup', 'signing-cleanup')) {
        return $false
    }
    try {
        $message = & $command
        Report PASS $case ($message -join ' ')
        return $true
    } catch {
        Report FAIL $case $_.Exception.Message
        return $false
    }
}

function FixtureValue($key) {
    $match = Select-String -LiteralPath $fixture -Pattern ('^' + $key + '\s*=\s*"([^"]*)"')
    if (@($match).Count -ne 1) { throw "Expected one $key in $fixture" }
    $value = $match.Matches[0].Groups[1].Value
    if ([string]::IsNullOrWhiteSpace($value)) { throw "Blank $key in $fixture" }
    return $value
}

function RunProcess($file, $arguments, $seconds, $stdout, $stderr, $directory) {
    $seconds = [Math]::Min($seconds, [Math]::Floor($deadline - $clock.Elapsed.TotalSeconds))
    if ($seconds -le 0) { return 124 }
    $redirect = @{}
    if ($stdout) { $redirect.RedirectStandardOutput = $stdout }
    if ($stderr) { $redirect.RedirectStandardError = $stderr }
    if ($directory) { $redirect.WorkingDirectory = $directory }
    $process = Start-Process -FilePath $file -ArgumentList $arguments -PassThru @redirect
    try {
        if (-not $process.WaitForExit($seconds * 1000)) {
            try { $process.Kill($true) } catch { Write-Warning 'Could not terminate a timed-out process tree.' }
            return 124
        }
        return $process.ExitCode
    } finally {
        $process.Dispose()
    }
}

function RunBundle($label, $destination) {
    $stdout = Join-Path $work "$config-$label.log"
    $stderr = Join-Path $work "$config-$label-stderr.log"
    $xtaskLogs = @('xtask.log', 'xtask.log.old', 'lifecycle.jsonl')
    foreach ($file in $xtaskLogs) {
        Remove-Item -LiteralPath (Join-Path $env:CARGO_TARGET_DIR $file) -ErrorAction SilentlyContinue
    }

    $status = 1
    try {
        $status = RunProcess 'cargo.exe' ('xtask bundle windows --package yard --config "{0}" --out "{1}"' -f $fixture, $destination) 1500 $stdout $stderr
    } catch {
        Write-Host "Could not start or wait for $label."
    }

    $files = @($stdout, $stderr)
    foreach ($file in $xtaskLogs) {
        $path = Join-Path $env:CARGO_TARGET_DIR $file
        if (Test-Path -LiteralPath $path) { $files += $path }
    }
    $files = @($files | Where-Object { Test-Path -LiteralPath $_ })
    $safe = $status -ne 124 -and (Test-Path -LiteralPath $stdout) -and (Test-Path -LiteralPath $stderr)
    if ($safe -and $env:WINDOWS_CERTIFICATE) {
        try {
            $patterns = @($env:WINDOWS_CERTIFICATE_PASSWORD, $env:WINDOWS_CERTIFICATE.Substring(0, 40))
            $safe = -not (Select-String -LiteralPath $files -SimpleMatch -Pattern $patterns -CaseSensitive)
        } catch {
            $safe = $false
        }
    }
    if ($safe) {
        Copy-Item -LiteralPath $stdout -Destination "$logs/$label.log"
        Copy-Item -LiteralPath $stderr -Destination "$logs/$label-stderr.log"
        foreach ($file in $xtaskLogs) {
            $path = Join-Path $env:CARGO_TARGET_DIR $file
            if (Test-Path -LiteralPath $path) {
                Copy-Item -LiteralPath $path -Destination (Join-Path $logs "$label-$file")
            }
        }
        Report PASS "$label-secrets" 'no signing credentials found in bundle logs'
    } else {
        Report FAIL "$label-secrets" 'bundle timed out, output was missing, a secret matched, or scan failed; logs withheld'
        return 1
    }
    return $status
}

function CheckBundle {
    $status = RunBundle 'bundle' $dist
    if ($status -ne 0) { throw "exit $status (see $config/bundle logs)" }
    if (-not (Test-Path -LiteralPath $setup)) { throw 'expected installer is missing' }
    if (-not (Test-Path -LiteralPath $zip)) { throw 'expected zip is missing' }
    return 'installer and zip created'
}

function RegistryValue($key, $valueName) {
    $entry = Get-Item -LiteralPath $key -ErrorAction SilentlyContinue
    if ($entry) { return $entry.GetValue($valueName) }
}

function CheckScheme {
    if ((RegistryValue $schemeKey '') -cne "URL:$name" -or (RegistryValue $schemeKey 'URL Protocol') -cne '') {
        throw 'scheme default or URL Protocol value differs'
    }
    return "$scheme is registered as URL:$name"
}

function CheckCommand {
    $actual = RegistryValue "$schemeKey\shell\open\command" ''
    if ($actual -cne $expectedCommand) { throw "expected $expectedCommand, got $actual" }
    return $actual
}

function CheckIcon {
    $expected = '"{0}",0' -f $exe
    $actual = RegistryValue "$schemeKey\DefaultIcon" ''
    if ($actual -cne $expected) { throw "expected $expected, got $actual" }
    return $actual
}

function CheckUninstallEntry {
    $entry = Get-ItemProperty -LiteralPath $uninstallKey
    if ($entry.DisplayName -cne "$name $version" -or $entry.DisplayVersion -cne $version -or
        $entry.InstallLocation.TrimEnd('\') -ne $appDir -or
        $entry.UninstallString.Trim('"') -ne (Join-Path $appDir 'unins000.exe')) {
        throw 'uninstall entry has unexpected name, version, location or command'
    }
    return "uninstall entry matches $identifier, $name and $version"
}

function CheckShortcut {
    if (-not (Test-Path -LiteralPath $shortcut)) { throw 'Start menu shortcut is missing' }
    $link = (New-Object -ComObject WScript.Shell).CreateShortcut($shortcut)
    if ($link.TargetPath -ne $exe -or $link.Arguments) { throw 'shortcut target or arguments differ' }
    return "Start menu shortcut targets $exe"
}

function CheckAppUserModelId {
    $folder = (New-Object -ComObject Shell.Application).NameSpace((Split-Path $shortcut))
    $item = $folder.ParseName((Split-Path $shortcut -Leaf))
    $actual = $item.ExtendedProperty('System.AppUserModel.ID')
    if ($actual -cne $identifier) { throw "expected $identifier, got $actual" }
    return $actual
}

function Install($label) {
    $installLog = Join-Path $logs "$label.log"
    $arguments = '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /LOG="{0}"' -f $installLog
    return RunProcess $setup $arguments 120
}

function CheckInstall {
    $status = Install 'install'
    if ($status -ne 0 -or -not (Test-Path -LiteralPath $exe)) {
        throw "exit $status or installed executable missing (see install.log)"
    }
    return "installed $exe"
}

function AppProcesses {
    Get-Process -Name $name -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $exe }
}

function WaitForWindow {
    for ($attempt = 0; $attempt -lt 30 -and $clock.Elapsed.TotalSeconds -lt $deadline; $attempt++) {
        $processes = @(AppProcesses)
        if ($processes.Count -eq 1 -and $processes[0].MainWindowHandle -ne 0) { return $processes[0] }
        Start-Sleep -Seconds 1
    }
    throw 'no single process with a window'
}

function CheckOpen {
    Start-Process -FilePath $exe
    $process = WaitForWindow
    return "opened a window at PID $($process.Id)"
}

function CheckScreenshot($path) {
    if (-not $path) { $path = Join-Path $logs 'window.png' }
    Add-Type -AssemblyName System.Windows.Forms, System.Drawing
    $bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $bitmap = [System.Drawing.Bitmap]::new($bounds.Width, $bounds.Height)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size)
        $bitmap.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    } finally {
        $graphics.Dispose()
        $bitmap.Dispose()
    }
    return 'captured the screen with the installed window'
}

function StopApp {
    AppProcesses | Stop-Process -Force -ErrorAction SilentlyContinue
    for ($attempt = 0; $attempt -lt 10 -and @(AppProcesses).Count; $attempt++) {
        Start-Sleep -Seconds 1
    }
}

function TraceCounts($path, $targetScheme) {
    $counts = & python "$PSScriptRoot/trace-counts.py" $path $targetScheme
    if ($LASTEXITCODE -ne 0 -or $counts -notmatch '^\d+ \d+ \d+$') { throw 'could not count URL trace groups' }
    return $counts
}

function OpenUrl($phase, $forwarded) {
    $before = (TraceCounts $trace $scheme).Split(' ')
    $expectedUrls = 1 + [int]$before[0]
    $expectedRequests = [int]$forwarded + [int]$before[1]
    $expectedForwarded = [int]$forwarded + [int]$before[2]
    $nonce = [Guid]::NewGuid().ToString('N')
    CheckScheme | Out-Null
    CheckCommand | Out-Null
    $effective = RegistryValue "Registry::HKEY_CLASSES_ROOT\$scheme\shell\open\command" ''
    $userChoice = RegistryValue "HKCU:\Software\Microsoft\Windows\Shell\Associations\UrlAssociations\$scheme\UserChoice" 'ProgId'
    if ($effective -cne $expectedCommand -or ($userChoice -and $userChoice -ne $scheme)) {
        throw 'the installed app is not the effective protocol handler'
    }
    $url = if ($phase -eq 'cold' -and $coldUrl) { $coldUrl } else { "${scheme}://$urlHost/$phase-$nonce" }
    $status = RunProcess $env:ComSpec ('/d /c start "" "{0}"' -f $url) 15
    if ($status -ne 0) { throw "start returned $status" }
    for ($attempt = 0; $attempt -lt 30 -and $clock.Elapsed.TotalSeconds -lt $deadline; $attempt++) {
        if ((TraceCounts $trace $scheme) -ceq "$expectedUrls $expectedRequests $expectedForwarded") { return }
        Start-Sleep -Seconds 1
    }
    throw "expected exactly one URL dispatch and $forwarded forwarded request(s) for $scheme"
}

function CheckColdUrl {
    if (@(AppProcesses).Count) { throw 'app was already running' }
    Remove-Item -LiteralPath $trace -ErrorAction SilentlyContinue
    OpenUrl 'cold' 0
    $process = WaitForWindow
    return "reached the app's URL handler at PID $($process.Id)"
}

function CheckWarmUrl($phase = 'warm') {
    $process = WaitForWindow
    OpenUrl $phase 1
    $current = @(AppProcesses)
    if ($current.Count -ne 1 -or $current[0].Id -ne $process.Id) {
        throw 'the original app process did not survive URL delivery'
    }
    return "reached the app's URL handler at the same PID $($process.Id)"
}

function CheckAppMutex {
    if (@(AppProcesses).Count -ne 1) { throw 'skipped: no single installed app process running' }
    $status = Install 'install-running'
    if ($status -ne 1 -or -not (Select-String -LiteralPath "$logs/install-running.log" -SimpleMatch "Setup has detected that $name is currently running" -Quiet)) {
        throw "expected the running-app refusal with exit 1, got exit $status (see install-running.log)"
    }
    return 'installer refused with exit 1 while the app was running'
}

function CheckUninstall {
    if (@(AppProcesses).Count) { throw 'skipped: the installed app did not stop' }
    $command = (Get-ItemProperty -LiteralPath $uninstallKey).UninstallString.Trim('"')
    if ($command -ne (Join-Path $appDir 'unins000.exe')) { throw 'unexpected uninstaller path' }
    $arguments = '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /LOG="{0}"' -f (Join-Path $logs 'uninstall.log')
    $status = RunProcess $command $arguments 120
    if ($status -ne 0) { throw "uninstaller exit $status" }
    for ($attempt = 0; $attempt -lt 60 -and $clock.Elapsed.TotalSeconds -lt $deadline -and (Test-Path -LiteralPath $appDir); $attempt++) {
        Start-Sleep -Seconds 1
    }
    if (Test-Path -LiteralPath $appDir) { throw 'install folder remains after waiting for uninstaller' }
    return 'silent uninstaller exited 0 and the install folder is gone'
}

function CheckUninstallKeys {
    if ((Test-Path -LiteralPath $uninstallKey) -or (Test-Path -LiteralPath "$schemeKey\DefaultIcon") -or
        (Test-Path -LiteralPath "$schemeKey\shell") -or $null -ne (RegistryValue $schemeKey '') -or
        $null -ne (RegistryValue $schemeKey 'URL Protocol')) {
        throw 'uninstall entry or app-owned scheme keys or values remain'
    }
    return 'uninstall entry and app-owned scheme registration removed'
}

function CheckUninstallFiles {
    if ((Test-Path -LiteralPath $appDir) -or (Test-Path -LiteralPath $shortcut)) {
        throw 'installed app folder or Start menu shortcut remains'
    }
    return 'installed app folder and Start menu shortcut removed'
}

function CheckUserData {
    foreach ($marker in $markers) {
        if ((Get-Content -LiteralPath $marker -Raw).Trim() -cne 'keep') { throw "user data changed at $marker" }
    }
    return 'local and roaming user data kept'
}

function CheckOtherSchemeValue {
    if ((RegistryValue $schemeKey 'OtherApp') -cne 'keep') { throw 'other program value is missing or changed' }
    return 'other program value under the scheme key kept'
}

function CopyAppLogs($label) {
    $directory = Split-Path $trace
    if (Test-Path -LiteralPath $directory) {
        Copy-Item -Recurse -LiteralPath $directory -Destination (Join-Path $logs $label)
    }
}

function CheckInstalledApp {
    $afterInstall = @('scheme', 'command', 'icon', 'uninstall-entry', 'shortcut', 'app-user-model-id',
        'open', 'screenshot', 'url-cold', 'url-warm', 'app-mutex', 'user-data-setup', 'uninstall',
        'uninstall-keys', 'uninstall-files', 'user-data', 'other-scheme-value')
    if (-not (Check bundle { CheckBundle })) {
        Skip 'bundle failed' (@('install') + $afterInstall)
        return
    }
    try {
        if (-not (Check install {
            New-Item -Path $schemeKey -Force | Out-Null
            New-ItemProperty -Path $schemeKey -Name 'OtherApp' -Value 'keep' -Force | Out-Null
            CheckInstall
        })) {
            Skip 'install failed' $afterInstall
            return
        }
        $schemeOk = Check scheme { CheckScheme }
        $commandOk = Check command { CheckCommand }
        [void](Check icon { CheckIcon })
        [void](Check uninstall-entry { CheckUninstallEntry })
        [void](Check shortcut { CheckShortcut })
        [void](Check app-user-model-id { CheckAppUserModelId })
        $opened = Check open { CheckOpen }
        if ($opened) {
            [void](Check screenshot { CheckScreenshot })
        } else {
            Skip 'open failed' @('screenshot')
        }
        StopApp
        CopyAppLogs 'direct-app-logs'
        if (-not ($schemeOk -and $commandOk)) {
            Skip 'the expected protocol handler is missing' @('url-cold', 'url-warm')
        } elseif (Check url-cold { CheckColdUrl }) {
            [void](Check url-warm { CheckWarmUrl })
        } else {
            Skip 'cold launch failed' @('url-warm')
        }
        if (-not @(AppProcesses).Count) {
            try { Start-Process -FilePath $exe; WaitForWindow | Out-Null }
            catch { Write-Host 'Could not start the app for the mutex check.' }
        }
        [void](Check app-mutex { CheckAppMutex })
        StopApp
        CopyAppLogs 'url-app-logs'
        $markers = @()
        $dataReady = Check user-data-setup {
            foreach ($base in @($env:LOCALAPPDATA, $env:APPDATA)) {
                $directory = Join-Path $base $name
                New-Item -ItemType Directory -Force $directory | Out-Null
                Set-Content -LiteralPath (Join-Path $directory 'runner-check-kept.txt') -Value 'keep'
            }
            'seeded local and roaming user data'
        }
        foreach ($base in @($env:LOCALAPPDATA, $env:APPDATA)) {
            $markers += Join-Path $base "$name/runner-check-kept.txt"
        }
        if (Check uninstall { CheckUninstall }) {
            [void](Check uninstall-keys { CheckUninstallKeys })
            [void](Check uninstall-files { CheckUninstallFiles })
            if ($dataReady) { [void](Check user-data { CheckUserData }) }
            else { Skip 'could not seed user data' @('user-data') }
            [void](Check other-scheme-value { CheckOtherSchemeValue })
        } else {
            Skip 'uninstall failed' @('uninstall-keys', 'uninstall-files', 'user-data', 'other-scheme-value')
        }
    } finally {
        StopApp
        Remove-ItemProperty -LiteralPath $schemeKey -Name 'OtherApp' -ErrorAction SilentlyContinue
    }
}

function NewTestCertificate {
    $certificate = New-SelfSignedCertificate -Type CodeSigningCert -Subject 'CN=YARD Runner Signing' -CertStoreLocation Cert:\CurrentUser\My
    $script:thumb = $certificate.Thumbprint
    $password = [Guid]::NewGuid().ToString('N')
    $pfx = Join-Path $work 'identity.pfx'
    $cer = Join-Path $work 'identity.cer'
    try {
        $securePassword = ConvertTo-SecureString $password -AsPlainText -Force
        Export-PfxCertificate -Cert $certificate -FilePath $pfx -Password $securePassword | Out-Null
        Export-Certificate -Cert $certificate -FilePath $cer | Out-Null
        Import-Certificate -FilePath $cer -CertStoreLocation Cert:\LocalMachine\Root | Out-Null
        Import-Certificate -FilePath $cer -CertStoreLocation Cert:\LocalMachine\TrustedPublisher | Out-Null
        Remove-Item -LiteralPath "Cert:\CurrentUser\My\$thumb" -DeleteKey
        $env:WINDOWS_CERTIFICATE = [Convert]::ToBase64String([IO.File]::ReadAllBytes($pfx))
        $env:WINDOWS_CERTIFICATE_PASSWORD = $password
    } finally {
        Remove-Item -LiteralPath $pfx, $cer -ErrorAction SilentlyContinue
    }
}

function CheckSignature($file) {
    $signature = Get-AuthenticodeSignature -LiteralPath $file
    $signer = $signature.SignerCertificate.Thumbprint
    $timestamp = $signature.TimeStamperCertificate.Thumbprint
    if ($signature.Status -ne 'Valid' -or $signature.SignatureType -ne 'Authenticode' -or
        $signer -ne $thumb -or -not $timestamp) {
        throw "status=$($signature.Status), type=$($signature.SignatureType), signer=$signer, timestamp=$timestamp"
    }
    return "Valid, signer $signer, timestamp certificate $timestamp"
}

function CheckZipExtract {
    New-Item -ItemType Directory (Join-Path $work 'unpacked') | Out-Null
    $arguments = '-xf "{0}" -C "{1}"' -f $zip, "$work/unpacked"
    $status = RunProcess "$env:SystemRoot\System32\tar.exe" $arguments 30 "$logs/zip-extract.log" "$logs/zip-extract-stderr.log"
    if ($status -ne 0 -or -not (Test-Path -LiteralPath $zipExe)) { throw 'see zip-extract.log' }
    return 'zip contains the expected executable'
}

function CheckTampered {
    $bytes = [IO.File]::ReadAllBytes($zipExe)
    $index = [int]($bytes.Length / 2)
    $bytes[$index] = $bytes[$index] -bxor 0xff
    $tampered = Join-Path $work 'tampered.exe'
    [IO.File]::WriteAllBytes($tampered, $bytes)
    $signature = Get-AuthenticodeSignature -LiteralPath $tampered
    if ($signature.Status -ne 'HashMismatch') { throw "expected HashMismatch, got $($signature.Status)" }
    return 'changed executable reports HashMismatch'
}

function CheckWrongPassword {
    $env:WINDOWS_CERTIFICATE_PASSWORD = [Guid]::NewGuid().ToString('N')
    $wrongOut = Join-Path $work 'wrong-password'
    $status = RunBundle 'wrong-password' $wrongOut
    $message = 'could not read WINDOWS_CERTIFICATE with WINDOWS_CERTIFICATE_PASSWORD'
    $bundleLogs = @("$logs/wrong-password.log", "$logs/wrong-password-stderr.log")
    $diagnostics = @(Select-String -LiteralPath $bundleLogs -SimpleMatch $message -CaseSensitive)
    $fixed = [regex]::Escape($message) + '(?:: [A-Za-z_]\w*(?:\.[A-Za-z_]\w*)+)?'
    if ($status -ne 1 -or (Test-Path -LiteralPath $wrongOut) -or $diagnostics.Count -eq 0) {
        throw "expected the fixed refusal and no output folder, got exit $status (see wrong-password logs)"
    }
    foreach ($diagnostic in $diagnostics) {
        if ($diagnostic.Line -cnotmatch ('^(?:(?:.*\bERROR \[.+\] failed at )?certificate: )?' + $fixed + '\z')) {
            throw 'unexpected wrong-password diagnostic (see wrong-password logs)'
        }
    }
    return 'fixed password refusal, no output folder'
}

function CheckSigningCleanup {
    if (Get-ChildItem -LiteralPath $env:TEMP -Filter 'xtask-signing-*' -Directory -Force) {
        throw 'xtask signing folders remain in TEMP'
    }
    $after = @(Get-ChildItem Cert:\CurrentUser\My | ForEach-Object Thumbprint)
    foreach ($entry in $after) {
        if ($entry -notin $userCertificates) { throw "unexpected certificate in CurrentUser\My: $entry" }
    }
    return 'no signing folders or added user certificates'
}

function RemoveTestTrust {
    if ($thumb) {
        $userLeaf = "Cert:\CurrentUser\My\$thumb"
        $trusted = @("Cert:\LocalMachine\Root\$thumb", "Cert:\LocalMachine\TrustedPublisher\$thumb")
        Remove-Item -LiteralPath $trusted -ErrorAction SilentlyContinue
        if (Test-Path -LiteralPath $userLeaf) {
            Remove-Item -LiteralPath $userLeaf -DeleteKey -ErrorAction SilentlyContinue
        }
        foreach ($path in @($userLeaf) + $trusted) {
            if (Test-Path -LiteralPath $path) { throw "test certificate remains at $path" }
        }
    }
    return 'test certificate absent from the machine trust stores and user store'
}

function CheckSignedArtifacts {
    [void](Check installer-signature { CheckSignature $setup })
    if (Check zip-extract { CheckZipExtract }) {
        [void](Check zip-signature { CheckSignature $zipExe })
        [void](Check tampered-signature { CheckTampered })
    } else {
        Skip 'zip extraction failed' @('zip-signature', 'tampered-signature')
    }
    if (Check install { CheckInstall }) {
        [void](Check installed-signature { CheckSignature $exe })
        [void](Check uninstaller-signature { CheckSignature "$appDir/unins000.exe" })
        [void](Check uninstall { CheckUninstall })
    } else {
        Skip 'signed install failed' @('installed-signature', 'uninstaller-signature', 'uninstall')
    }
}

$loadConfig = {
    param($configName)
    $config = $configName
    $logs = Join-Path $checks $config
    New-Item -ItemType Directory -Force $logs | Out-Null
    $fixture = Join-Path $PWD "crates/yard_build_config/tests/configs/$config/product.toml"
    $name = FixtureValue 'name'
    $version = FixtureValue 'version'
    $scheme = FixtureValue 'url_scheme'
    $identifier = FixtureValue 'identifier'
    $dist = Join-Path $work "dist-$config"
    $appDir = Join-Path $env:LOCALAPPDATA "Programs\$name"
    $exe = Join-Path $appDir "$name.exe"
    $shortcut = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\$name.lnk"
    $schemeKey = "HKCU:\Software\Classes\$scheme"
    $uninstallKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\${identifier}_is1"
    $expectedCommand = '"{0}" "%1"' -f $exe
    $trace = Join-Path $env:LOCALAPPDATA "$name\logs\lifecycle.jsonl"
    $lifecycleVariable = ($name.ToUpperInvariant() -creplace '[^A-Z0-9]', '_') + '_LIFECYCLE'
    $architecture = if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { 'aarch64' } else { 'x86_64' }
    $setup = Join-Path $dist "$name-$version-$architecture-setup.exe"
    $zip = Join-Path $dist "$name-$version-windows-$architecture.zip"

}

function CheckCoexistence {
    $started = @{}
    $configured = @()
    $activeTraces = @{}
    try {
        foreach ($product in @('plain', 'other')) {
            . $loadConfig $product
            $configured += $product
            [Environment]::SetEnvironmentVariable($lifecycleVariable, 'info,yard_shell=debug', 'Process')
            if ((Install 'coexist-install') -ne 0) { throw "could not install $product for coexistence" }
            [void](CheckOpen)
            $started[$product] = (WaitForWindow).Id
            $activeTraces[$product] = $trace
        }
        if ($started.plain -eq $started.other) { throw 'expected two distinct app processes' }
        foreach ($product in @('plain', 'other')) {
            . $loadConfig $product
            $peer = if ($product -eq 'plain') { 'other' } else { 'plain' }
            $before = TraceCounts $activeTraces[$peer] '*'
            [void](CheckWarmUrl "coexist-$product")
            if ((TraceCounts $activeTraces[$peer] '*') -cne $before) {
                throw "the peer's URL trace also changed after sending to $product"
            }
        }
        foreach ($product in @('plain', 'other')) {
            . $loadConfig $product
            $processes = @(AppProcesses)
            if ($processes.Count -ne 1 -or $processes[0].Id -ne $started[$product]) {
                throw "$product did not retain its original process"
            }
        }
        foreach ($product in @('plain', 'other')) {
            Copy-Item -LiteralPath $activeTraces[$product] -Destination "$checks/$product/coexist-trace.jsonl"
        }
        return "two PIDs; each URL reached its own app's URL handler only"
    } finally {
        foreach ($product in $configured) {
            . $loadConfig $product
            StopApp
            [Environment]::SetEnvironmentVariable($lifecycleVariable, $null, 'Process')
            if (Test-Path -LiteralPath $uninstallKey) { [void](CheckUninstall) }
        }
    }
}

# A developer's application, made with `cargo xtask new-app` in a fresh
# checkout, installed beside the sample bundled there with `--package yard`.
# Each starts with its own home and lifecycle variables.
$fresh = Join-Path $work 'fresh'
$northStarIcon = 'crates/yard_build_config/tests/configs/other/icon.png'

function TraceFact($path, $key) {
    $value = & python "$PSScriptRoot/trace-facts.py" $path $key
    if ($LASTEXITCODE -ne 0) { throw "could not read $key from $path" }
    return [string]$value
}

# Loads the fresh checkout's product from app/<package>/, with its files under <label>.
$loadFresh = {
    param($package, $label)
    $fixture = Join-Path $fresh "app/$package/product.toml"
    $logs = Join-Path $checks "$config/$label"
    New-Item -ItemType Directory -Force $logs | Out-Null
    $name = FixtureValue 'name'
    $version = FixtureValue 'version'
    $scheme = FixtureValue 'url_scheme'
    $identifier = FixtureValue 'identifier'
    $dist = Join-Path $work "$config-$label"
    $appHome = Join-Path $work "$config-$label-home"
    $appDir = Join-Path $env:LOCALAPPDATA "Programs\$name"
    $exe = Join-Path $appDir "$name.exe"
    $schemeKey = "HKCU:\Software\Classes\$scheme"
    $uninstallKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\${identifier}_is1"
    $expectedCommand = '"{0}" "%1"' -f $exe
    $trace = Join-Path $appHome 'logs\lifecycle.jsonl'
    $variablePrefix = $name.ToUpperInvariant() -creplace '[^A-Z0-9]', '_'
    $architecture = if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { 'aarch64' } else { 'x86_64' }
    $setup = Join-Path $dist "$name-$version-$architecture-setup.exe"
}

function CheckNewApp {
    $log = Join-Path $checks 'northstar/new-app'
    $status = RunProcess 'git' ('clone --quiet --no-local "{0}" "{1}"' -f $PWD, $fresh) 300 "$log-clone.log" "$log-clone-stderr.log"
    if ($status -ne 0) { throw "git clone exit $status (see northstar/new-app-clone logs)" }
    $arguments = 'xtask new-app --name "North Star" --identifier test.yard.northstar --icon "{0}"' -f (Join-Path $fresh $northStarIcon)
    $status = RunProcess 'cargo.exe' $arguments 900 "$log.log" "$log-stderr.log" $fresh
    if ($status -ne 0) { throw "new-app exit $status (see northstar/new-app logs)" }
    $fixture = Join-Path $fresh 'app/north-star/product.toml'
    if (-not (Test-Path -LiteralPath $fixture)) { throw 'app/north-star/product.toml is missing' }
    if ((FixtureValue 'name') -cne 'North Star' -or (FixtureValue 'identifier') -cne 'test.yard.northstar' -or
        (FixtureValue 'url_scheme') -cne 'northstar') {
        throw 'app/north-star/product.toml does not hold the name, identifier, and scheme given'
    }
    if (-not (Select-String -LiteralPath (Join-Path $fresh 'Cargo.toml') -Quiet -CaseSensitive `
        -Pattern '^default-members\s*=\s*\[\s*"app/north-star"\s*,?\s*\]')) {
        throw 'default-members does not name app/north-star'
    }
    return 'app/north-star/ made in a fresh checkout; default-members names it'
}

function FreshBundle($arguments) {
    $stdout = Join-Path $logs 'bundle.log'
    $stderr = Join-Path $logs 'bundle-stderr.log'
    $xtaskLogs = @('xtask.log', 'xtask.log.old', 'lifecycle.jsonl')
    foreach ($file in $xtaskLogs) {
        Remove-Item -LiteralPath (Join-Path $env:CARGO_TARGET_DIR $file) -ErrorAction SilentlyContinue
    }
    $status = RunProcess 'cargo.exe' ('xtask bundle windows {0}--out "{1}"' -f $arguments, $dist) 1500 $stdout $stderr $fresh
    foreach ($file in $xtaskLogs) {
        $path = Join-Path $env:CARGO_TARGET_DIR $file
        if (Test-Path -LiteralPath $path) { Copy-Item -LiteralPath $path -Destination (Join-Path $logs "bundle-$file") }
    }
    if ($status -ne 0) { throw "exit $status (see $config bundle logs)" }
    if (-not (Test-Path -LiteralPath $setup)) { throw 'expected installer is missing' }
    return "created $(Split-Path $setup -Leaf)"
}

function CheckProductName {
    $actual = (Get-Item -LiteralPath $exe).VersionInfo.ProductName
    if ($actual -cne $name) { throw "expected $name, got $actual" }
    return "ProductName = $actual"
}

# The image drawn at the size of the icon Windows extracts from an .exe.
function IconBitmap($image) {
    $bitmap = [System.Drawing.Bitmap]::new(32, 32)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
        $graphics.DrawImage($image, 0, 0, 32, 32)
    } finally {
        $graphics.Dispose()
    }
    return $bitmap
}

function IconDistance($a, $b) {
    $sum = 0
    for ($y = 0; $y -lt 32; $y++) {
        for ($x = 0; $x -lt 32; $x++) {
            $p = $a.GetPixel($x, $y)
            $q = $b.GetPixel($x, $y)
            $sum += [Math]::Abs([int]$p.A - $q.A) + [Math]::Abs([int]$p.R - $q.R) +
                [Math]::Abs([int]$p.G - $q.G) + [Math]::Abs([int]$p.B - $q.B)
        }
    }
    return $sum
}

# The .exe's icon is nearer the icon given to new-app than the sample's, once
# both are drawn at the extracted icon's size.
function CheckExeIcon {
    Add-Type -AssemblyName System.Drawing
    $images = @()
    try {
        $icon = [System.Drawing.Icon]::ExtractAssociatedIcon($exe)
        $images += $icon
        $extracted = $icon.ToBitmap()
        $images += $extracted
        $actual = IconBitmap $extracted
        $images += $actual
        $actual.Save((Join-Path $logs 'exe-icon.png'), [System.Drawing.Imaging.ImageFormat]::Png)
        $given = [System.Drawing.Image]::FromFile((Join-Path $fresh $northStarIcon))
        $images += $given
        $sampleIcon = [System.Drawing.Image]::FromFile((Join-Path $fresh 'app/yard/icon.png'))
        $images += $sampleIcon
        $givenSmall = IconBitmap $given
        $images += $givenSmall
        $sampleSmall = IconBitmap $sampleIcon
        $images += $sampleSmall
        $own = IconDistance $actual $givenSmall
        $foreign = IconDistance $actual $sampleSmall
        $between = IconDistance $givenSmall $sampleSmall
    } finally {
        foreach ($image in $images) { $image.Dispose() }
    }
    # Nearer the given icon than the sample's is not enough: a third icon, such
    # as a fallback, could be. It must also be within a fifth of the distance
    # that tells the two icons apart.
    $distances = "distance to the given icon $own, to the sample's $foreign, between the two $between"
    if ($own -ge $foreign -or 5 * $own -ge $between) {
        throw "the .exe's icon is not the one given to new-app ($distances; needs the first under a fifth of the last)"
    }
    return "the .exe's icon is the one given to new-app ($distances)"
}

# The loaded product's trace names it, and holds exactly $count URLs refused
# as `no such extension`.
function CheckOwnTrace($count) {
    if ((TraceFact $trace 'name') -cne $name -or (TraceFact $trace 'identifier') -cne $identifier -or
        (TraceFact $trace 'no-such-extension') -cne "$count") {
        throw "$trace is not $name's with $count URL(s) refused as no such extension"
    }
}

function SetProductEnvironment($lifecycle) {
    $directory = if ($lifecycle) { $appHome } else { $null }
    [Environment]::SetEnvironmentVariable("${variablePrefix}_HOME", $directory, 'Process')
    [Environment]::SetEnvironmentVariable("${variablePrefix}_LIFECYCLE", $lifecycle, 'Process')
}

# Seconds to wait before counts are read again, to catch a late delivery.
$settle = 3

function CheckFreshColdUrl {
    $message = CheckColdUrl
    Start-Sleep -Seconds $settle
    CheckOwnTrace 1
    return "${scheme}://check $message, refused once as no such extension"
}

function CheckFreshCoexistence {
    . $loadFresh 'north-star' 'north-star'
    $north = @{ pid = (WaitForWindow).Id; trace = $trace; home = $appHome; dir = $appDir; scheme = $scheme
        mutex = "$identifier-instance" }
    $before = TraceCounts $north.trace '*'
    . $loadFresh 'yard' 'sample'
    [void](CheckColdUrl)
    Start-Sleep -Seconds $settle
    CheckOwnTrace 1
    if ((TraceCounts $north.trace '*') -cne $before) { throw 'yard://check of the cold start also reached North Star' }
    $sample = @{ pid = (WaitForWindow).Id; trace = $trace; home = $appHome; dir = $appDir; scheme = $scheme
        mutex = "$identifier-instance" }
    if ($north.pid -eq $sample.pid) { throw 'expected one North Star and one sample process' }

    # Each delivery is sampled again after a pause: the destination still has
    # exactly one more URL, and the peer none.
    . $loadFresh 'north-star' 'north-star'
    $before = TraceCounts $sample.trace '*'
    [void](CheckWarmUrl 'coexist-north')
    Start-Sleep -Seconds $settle
    CheckOwnTrace 2
    if ((TraceCounts $sample.trace '*') -cne $before) { throw 'northstar://check also reached the sample' }
    [void](CheckCommand)
    . $loadFresh 'yard' 'sample'
    $before = TraceCounts $north.trace '*'
    [void](CheckWarmUrl 'coexist-sample')
    Start-Sleep -Seconds $settle
    CheckOwnTrace 2
    if ((TraceCounts $north.trace '*') -cne $before) { throw 'yard://check also reached North Star' }
    [void](CheckCommand)
    foreach ($product in @(@('north-star', 'north-star', $north), @('yard', 'sample', $sample))) {
        . $loadFresh $product[0] $product[1]
        $processes = @(AppProcesses)
        if ($processes.Count -ne 1 -or $processes[0].Id -ne $product[2].pid) {
            throw "$name did not keep its original process"
        }
    }

    $northDir = $north.dir.TrimEnd('\') + '\'
    $sampleDir = $sample.dir.TrimEnd('\') + '\'
    if ($northDir.StartsWith($sampleDir, 'OrdinalIgnoreCase') -or $sampleDir.StartsWith($northDir, 'OrdinalIgnoreCase')) {
        throw "install folders meet: $($north.dir) and $($sample.dir)"
    }
    foreach ($product in @($north, $sample)) {
        $data = TraceFact $product.trace 'data'
        if (-not $data -or [IO.Path]::GetFullPath($data) -ne [IO.Path]::GetFullPath((Join-Path $product.home 'data'))) {
            throw "data folder $data is not under its own home $($product.home)"
        }
    }
    # The running apps' pipe and mutex share one name, which holds a hash of
    # the home given; the installer's AppMutex is the name without it.
    $northPipe = TraceFact $north.trace 'socket'
    $samplePipe = TraceFact $sample.trace 'socket'
    if (-not $northPipe -or -not $samplePipe -or $northPipe -eq $samplePipe) {
        throw "instance pipes: North Star $northPipe, sample $samplePipe"
    }
    if ($north.mutex -eq $sample.mutex) {
        throw "installer AppMutex names: North Star $($north.mutex), sample $($sample.mutex)"
    }
    if ($north.scheme -eq $sample.scheme) { throw "both register $($north.scheme)" }
    $northMenu = TraceFact $north.trace 'menu'
    $sampleMenu = TraceFact $sample.trace 'menu'
    if ($northMenu -cne 'North Star' -or $sampleMenu -cne 'yard') {
        throw "application menus: North Star $northMenu, sample $sampleMenu"
    }
    return ("PIDs $($north.pid) and $($sample.pid); own install folder, data folder, scheme, and menu; " +
        "running instance pipes $northPipe and $samplePipe (from the traces); installer AppMutex " +
        "$($north.mutex) and $($sample.mutex) (derived from each identifier as bundle windows writes it, not read " +
        "from the installers); each URL reached its own app only, re-read after ${settle}s")
}

function UpdateGit($directory, [string[]]$gitArguments) {
    $output = & git -C $directory @gitArguments
    if ($LASTEXITCODE -ne 0) { throw "git $($gitArguments[0]) exit $LASTEXITCODE" }
    return $output
}

function CheckUpdate {
    $fresh = Join-Path $work 'update-clone'
    $checkout = $PWD.Path
    $ref = "refs/tags/yard-runner-update-$(Split-Path $work -Leaf)"
    $urlHost = 'check'
    $coldUrl = 'upstreamstar://check'
    $updateLogs = Join-Path $checks 'update'
    New-Item -ItemType Directory -Force $updateLogs | Out-Null

    for ($index = 0; $index -lt 5; $index++) {
        $case = $updateCases[$index]
        try {
            switch ($case) {
                'update-clone' {
                    $target = UpdateGit $checkout @('rev-parse', 'HEAD')
                    $arguments = 'clone --quiet --no-local "{0}" "{1}"' -f $checkout, $fresh
                    $status = RunProcess 'git' $arguments 300 "$updateLogs/clone.log" "$updateLogs/clone-stderr.log"
                    if ($status -ne 0) { throw "git clone exit $status (see update/clone logs)" }
                    UpdateGit $fresh @('checkout', '-B', 'upstream-star-app', "$target~1") | Out-Null
                    $detail = "cloned YARD and checked out $target~1"
                }
                'update-new-app' {
                    # new-app resolves offline, and the job fetched the newer lockfile.
                    $status = RunProcess 'cargo.exe' 'fetch --locked' 900 "$updateLogs/fetch.log" "$updateLogs/fetch-stderr.log" $fresh
                    if ($status -ne 0) { throw "cargo fetch exit $status (see update/fetch logs)" }
                    $arguments = 'xtask new-app --name "Upstream Star" --identifier test.yard.upstreamstar --scheme upstreamstar --icon "{0}"' -f (Join-Path $fresh $northStarIcon)
                    $status = RunProcess 'cargo.exe' $arguments 900 "$updateLogs/new-app.log" "$updateLogs/new-app-stderr.log" $fresh
                    if ($status -ne 0) { throw "new-app exit $status (see update/new-app logs)" }
                    . $loadFresh 'upstream-star' 'upstream-star'
                    if ($name -cne 'Upstream Star' -or $identifier -cne 'test.yard.upstreamstar' -or $scheme -cne 'upstreamstar') {
                        throw 'app/upstream-star/product.toml does not hold the given name, identifier, and scheme'
                    }
                    UpdateGit $fresh @('config', 'user.name', 'YARD Runner')
                    UpdateGit $fresh @('config', 'user.email', 'runner@example.invalid')
                    UpdateGit $fresh @('add', '--', 'Cargo.toml', 'Cargo.lock', 'app/upstream-star')
                    UpdateGit $fresh @('commit', '-m', 'Create Upstream Star') | Out-Null
                    $start = UpdateGit $fresh @('rev-parse', 'HEAD')
                    $detail = "committed Upstream Star and Cargo.lock in $start"
                }
                'update' {
                    $branch = "yard-update/$($target.Substring(0, 12))"
                    UpdateGit $checkout @('update-ref', $ref, $target, ('0' * $target.Length))
                    try {
                        $arguments = 'xtask update --remote "{0}" --ref {1}' -f $checkout, $ref
                        $status = RunProcess 'cargo.exe' $arguments 1500 "$updateLogs/update.log" "$updateLogs/update-stderr.log" $fresh
                        if ($status -ne 0) { throw "update exit $status (see update/update logs)" }
                        $actualBranch = UpdateGit $fresh @('branch', '--show-current')
                        if ($actualBranch -cne $branch) { throw "expected branch $branch, got $actualBranch" }
                    } finally {
                        UpdateGit $checkout @('update-ref', '-d', $ref, $target)
                    }
                    $detail = "updated to YARD $target on $branch"
                }
                'update-app-kept' {
                    UpdateGit $fresh @('diff', '--exit-code', '--quiet', $start, 'HEAD', '--', 'app/upstream-star')
                    $detail = 'app/upstream-star matches the pre-update commit'
                }
                'update-manifest' {
                    $manifest = UpdateGit $fresh @('show', 'HEAD:Cargo.toml')
                    if (-not ($manifest -cmatch '^default-members\s*=\s*\[\s*"app/upstream-star"\s*,?\s*\]')) {
                        throw 'HEAD:Cargo.toml does not set app/upstream-star as the default member'
                    }
                    $detail = 'HEAD:Cargo.toml sets app/upstream-star as the default member'
                }
            }
            Report PASS $case $detail
        } catch {
            Report FAIL $case $_.Exception.Message
            Skip "$case failed" $updateCases[($index + 1)..($updateCases.Count - 1)]
            return
        }
    }

    if (-not (Check bundle { FreshBundle '' })) {
        Skip 'bundle failed' $updateCases[6..($updateCases.Count - 1)]
        return
    }
    SetProductEnvironment 'info,yard_shell=debug,yard_paths=debug'
    try {
        if (-not (Check install { CheckInstall })) {
            Skip 'install failed' $updateCases[7..($updateCases.Count - 1)]
            return
        }
        [void](Check product-name { CheckProductName })
        [void](Check icon { CheckExeIcon })
        [void](Check scheme { "$(CheckScheme); $(CheckCommand)" })
        [void](Check url {
            $detail = CheckFreshColdUrl
            if ((TraceCounts $trace $scheme) -cne '1 0 0') { throw 'expected exactly one dispatch in the installed app' }
            $detail
        })
    } finally {
        StopApp
        CopyAppLogs 'app-logs'
        SetProductEnvironment $null
        if (Test-Path -LiteralPath $uninstallKey) {
            try { [void](CheckUninstall) } catch { Write-Host "Could not uninstall $name." }
        }
    }
}

function CheckNorthStar {
    $urlHost = 'check'
    New-Item -ItemType Directory -Force (Join-Path $checks 'northstar') | Out-Null
    if (-not (Check new-app { CheckNewApp })) {
        Skip 'new-app failed' $northStarCases[1..($northStarCases.Count - 1)]
        return
    }
    try {
        . $loadFresh 'north-star' 'north-star'
        # No --package: the default member is the developer's application.
        $northReady = Check bundle { FreshBundle '' }
        . $loadFresh 'yard' 'sample'
        $sampleReady = Check sample-bundle { FreshBundle '--package yard ' }
        SetProductEnvironment 'info,yard_shell=debug,yard_paths=debug'
        . $loadFresh 'north-star' 'north-star'
        SetProductEnvironment 'info,yard_shell=debug,yard_paths=debug'
        if (-not $northReady) {
            Skip 'bundle failed' @('install', 'product-name', 'icon', 'scheme')
        } elseif (Check install { CheckInstall }) {
            [void](Check product-name { CheckProductName })
            [void](Check icon { CheckExeIcon })
            [void](Check scheme { "$(CheckScheme); $(CheckCommand)" })
        } else {
            $northReady = $false
            Skip 'install failed' @('product-name', 'icon', 'scheme')
        }
        . $loadFresh 'yard' 'sample'
        if (-not $sampleReady) {
            Skip 'sample bundle failed' @('sample-install')
        } else {
            $sampleReady = Check sample-install { CheckInstall }
        }

        . $loadFresh 'north-star' 'north-star'
        $running = $false
        if (-not $northReady) {
            Skip 'install failed' @('url', 'screenshot')
        } elseif (Check url { CheckFreshColdUrl }) {
            $running = $true
            [void](Check screenshot { CheckScreenshot (Join-Path $checks 'northstar/north-star/window.png') })
        } else {
            Skip 'url failed' @('screenshot')
        }
        if (-not ($running -and $sampleReady)) {
            Skip 'North Star did not run or the sample was not installed' @('coexistence')
        } else {
            [void](Check coexistence { CheckFreshCoexistence })
        }
    } finally {
        foreach ($product in @(@('north-star', 'north-star'), @('yard', 'sample'))) {
            . $loadFresh $product[0] $product[1]
            StopApp
            CopyAppLogs 'app-logs'
            SetProductEnvironment $null
            if (Test-Path -LiteralPath $uninstallKey) {
                try { [void](CheckUninstall) } catch { Write-Host "Could not uninstall $name." }
            }
        }
    }
}

try {
    foreach ($config in @('plain', 'spaced', 'other', 'signed')) {
        if ($clock.Elapsed.TotalSeconds -ge $deadline) { break }
        try {
            . $loadConfig $config

            if ($config -ne 'signed') {
                [Environment]::SetEnvironmentVariable($lifecycleVariable, 'info,yard_shell=debug', 'Process')
                try { CheckInstalledApp }
                finally { [Environment]::SetEnvironmentVariable($lifecycleVariable, $null, 'Process') }
                continue
            }

            $zipExe = Join-Path $work "unpacked\$name\$name.exe"
            $thumb = $null
            $userCertificates = @(Get-ChildItem Cert:\CurrentUser\My | ForEach-Object Thumbprint)
            try {
                if (Check certificate {
                    try { NewTestCertificate } catch { throw 'could not create, export or trust the test identity' }
                    'temporary test certificate trusted for this run'
                }) {
                    $signedBundled = Check bundle { CheckBundle }
                    [void](Check signing-cleanup-after-bundle { CheckSigningCleanup })
                    if ($signedBundled) { CheckSignedArtifacts }
                    else { Skip 'signed bundle failed' $signedArtifactCases }
                    [void](Check wrong-password { CheckWrongPassword })
                } else {
                    Skip 'certificate setup failed' (@('bundle', 'bundle-secrets', 'signing-cleanup-after-bundle',
                        'wrong-password', 'wrong-password-secrets') + $signedArtifactCases)
                }
            } finally {
                Remove-Item Env:WINDOWS_CERTIFICATE, Env:WINDOWS_CERTIFICATE_PASSWORD -ErrorAction SilentlyContinue
                [void](Check trust-cleanup { RemoveTestTrust })
                [void](Check signing-cleanup { CheckSigningCleanup })
            }
        } catch {
            $reason = $_.Exception.Message
            foreach ($case in $expected[$config]) {
                if ($case -ne 'coexistence' -and -not $results["$config $case"]) {
                    Report FAIL $case "skipped: $reason"
                }
            }
            $script:recordFailed = $true
        }
    }
    $config = 'plain'
    [void](Check coexistence { CheckCoexistence })
    $config = 'northstar'
    try {
        CheckNorthStar
    } catch {
        $reason = $_.Exception.Message
        foreach ($case in $northStarCases) {
            if (-not $results["northstar $case"]) { Report FAIL $case "skipped: $reason" }
        }
    }
    $config = 'update'
    try {
        CheckUpdate
    } catch {
        $reason = $_.Exception.Message
        foreach ($case in $updateCases) {
            if (-not $results["update $case"]) { Report FAIL $case "skipped: $reason" }
        }
    }
} finally {
    Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
    foreach ($config in @('plain', 'spaced', 'other', 'signed', 'northstar', 'update')) {
        foreach ($case in $expected[$config]) {
            $count = $results["$config $case"]
            if (-not $count) {
                $detail = if ($clock.Elapsed.TotalSeconds -ge $deadline) { 'no result (script deadline reached)' } else { 'no result' }
                Report FAIL $case $detail
            } elseif ($count -ne 1) {
                Report FAIL $case 'duplicate results'
            }
        }
    }
    try {
        if (Select-String -LiteralPath $report -Pattern '^FAIL ' -Quiet) { $script:recordFailed = $true }
    } catch {
        $script:recordFailed = $true
    }
    Add-Content -LiteralPath $report -Value 'DONE windows'
}

if ($recordFailed) { exit 1 }
exit 0
