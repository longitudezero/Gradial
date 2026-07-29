param(
    [Parameter(Mandatory = $true)]
    [string]$EventPath,

    [Parameter(Mandatory = $true)]
    [string]$ReadyPath,

    [Parameter(Mandatory = $true)]
    [string]$WindowTitle,

    [ValidateSet("normal", "maximized")]
    [string]$InitialState = "normal"
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

public sealed class GradialWindowSmokeTarget : Form
{
    private readonly string eventPath;
    private readonly string readyPath;
    private readonly bool startsMaximized;

    [DllImport("user32.dll")]
    private static extern bool SetForegroundWindow(IntPtr window);

    public GradialWindowSmokeTarget(
        string eventPath,
        string readyPath,
        string windowTitle,
        bool startsMaximized
    )
    {
        this.eventPath = eventPath;
        this.readyPath = readyPath;
        this.startsMaximized = startsMaximized;
        Text = windowTitle;
        Width = 480;
        Height = 180;
        StartPosition = FormStartPosition.CenterScreen;
        Controls.Add(new Label {
            AutoSize = true,
            Left = 18,
            Top = 22,
            Text = "Temporary target for Gradial window-command validation"
        });
        Shown += (_, __) => {
            if (this.startsMaximized) WindowState = FormWindowState.Maximized;
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
        const int WmClose = 0x0010;
        const int WmSysCommand = 0x0112;
        if (message.Msg == WmClose) {
            File.AppendAllText(eventPath, "close" + Environment.NewLine);
        } else if (message.Msg == WmSysCommand) {
            long command = message.WParam.ToInt64() & 0xFFF0L;
            if (
                command == 0xF020L ||
                command == 0xF030L ||
                command == 0xF120L
            ) {
                File.AppendAllText(
                    eventPath,
                    "system," + command + Environment.NewLine
                );
            }
        }
        base.WndProc(ref message);
    }
}
"@

[System.Windows.Forms.Application]::EnableVisualStyles()
$startsMaximized = $InitialState -eq "maximized"
$form = [GradialWindowSmokeTarget]::new(
    $EventPath,
    $ReadyPath,
    $WindowTitle,
    $startsMaximized
)
[System.Windows.Forms.Application]::Run($form)
