# Spike S3 fixture: a XAML (WPF) TextBox above a PasswordBox. The window title
# carries the length of each field ("s3 t=<n> p=<n>"), never its text. The
# window closes when the stop file exists.
param([string]$StopFile)
Add-Type -AssemblyName PresentationFramework
[xml]$xaml = @"
<Window xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
        Title="s3 t=0 p=0" Left="1160" Top="40" Width="540" Height="300"
        WindowStartupLocation="Manual">
  <StackPanel>
    <TextBox Name="plain" Height="110" FontSize="20" Margin="8"/>
    <PasswordBox Name="secret" Height="110" FontSize="20" Margin="8"/>
  </StackPanel>
</Window>
"@
$window = [Windows.Markup.XamlReader]::Load((New-Object System.Xml.XmlNodeReader $xaml))
$plain = $window.FindName("plain")
$secret = $window.FindName("secret")
$update = { $window.Title = "s3 t=$($plain.Text.Length) p=$($secret.Password.Length)" }
$plain.Add_TextChanged($update)
$secret.Add_PasswordChanged($update)
$timer = New-Object Windows.Threading.DispatcherTimer
$timer.Interval = [TimeSpan]::FromMilliseconds(200)
$timer.Add_Tick({ if (Test-Path $StopFile) { $window.Close() } })
$timer.Start()
$window.ShowDialog() | Out-Null
