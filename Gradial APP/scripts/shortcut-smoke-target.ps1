param(
    [Parameter(Mandatory = $true)]
    [string]$EventPath,

    [Parameter(Mandatory = $true)]
    [string]$ReadyPath
)

$ErrorActionPreference = "Stop"

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type -ReferencedAssemblies @("System.Windows.Forms", "System.Drawing") -TypeDefinition @"
using System;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Windows.Forms;

public sealed class GradialShortcutSmokeTarget : Form
{
    private readonly string eventPath;
    private readonly string readyPath;

    [DllImport("user32.dll")]
    private static extern bool SetForegroundWindow(IntPtr window);

    public GradialShortcutSmokeTarget(string eventPath, string readyPath)
    {
        this.eventPath = eventPath;
        this.readyPath = readyPath;
        Text = "Gradial Shortcut Smoke Target";
        Width = 360;
        Height = 120;
        StartPosition = FormStartPosition.CenterScreen;
        FormBorderStyle = FormBorderStyle.FixedToolWindow;
        Controls.Add(new Label {
            AutoSize = true,
            Left = 18,
            Top = 22,
            Text = "Temporary target for Gradial shortcut validation"
        });
        Shown += (_, __) => {
            Activate();
            SetForegroundWindow(Handle);
            File.WriteAllText(
                readyPath,
                Process.GetCurrentProcess().Id + "," + Handle.ToInt64()
            );
        };
    }

    protected override void WndProc(ref Message message)
    {
        const int WmKeyDown = 0x0100;
        const int WmKeyUp = 0x0101;
        const int WmSysKeyDown = 0x0104;
        const int WmSysKeyUp = 0x0105;
        if (
            message.Msg == WmKeyDown ||
            message.Msg == WmKeyUp ||
            message.Msg == WmSysKeyDown ||
            message.Msg == WmSysKeyUp
        ) {
            long encoded = message.LParam.ToInt64();
            int scanCode = (int)((encoded >> 16) & 0xFF);
            bool extended = ((encoded >> 24) & 1) != 0;
            bool keyUp = message.Msg == WmKeyUp || message.Msg == WmSysKeyUp;
            File.AppendAllText(
                eventPath,
                message.Msg + "," + message.WParam.ToInt64() + "," + scanCode + "," +
                extended.ToString().ToLowerInvariant() + "," +
                keyUp.ToString().ToLowerInvariant() + Environment.NewLine
            );
        }
        base.WndProc(ref message);
    }
}
"@

[System.Windows.Forms.Application]::EnableVisualStyles()
$form = [GradialShortcutSmokeTarget]::new($EventPath, $ReadyPath)
[System.Windows.Forms.Application]::Run($form)
