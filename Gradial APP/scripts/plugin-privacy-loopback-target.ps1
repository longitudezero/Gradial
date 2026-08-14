param(
    [Parameter(Mandatory = $true)]
    [ValidateRange(1, 65535)]
    [int]$Port,

    [Parameter(Mandatory = $true)]
    [string]$RequestPath,

    [Parameter(Mandatory = $true)]
    [string]$ReadyPath
)

$ErrorActionPreference = "Stop"

$listener = [Net.Sockets.TcpListener]::new(
    [Net.IPAddress]::Loopback,
    $Port
)
$client = $null
$stream = $null
try {
    $listener.Start(1)
    [IO.File]::WriteAllText(
        $ReadyPath,
        "ready",
        [Text.UTF8Encoding]::new($false)
    )
    $client = $listener.AcceptTcpClient()
    $client.ReceiveTimeout = 1000
    $client.SendTimeout = 1000
    $stream = $client.GetStream()
    $buffer = New-Object byte[] 4096
    $memory = [IO.MemoryStream]::new()
    try {
        $headerEnd = -1
        do {
            $read = $stream.Read($buffer, 0, $buffer.Length)
            if ($read -le 0) {
                throw "The loopback request closed before its headers completed."
            }
            $memory.Write($buffer, 0, $read)
            $bytes = $memory.ToArray()
            for ($index = [Math]::Max(0, $bytes.Length - $read - 3); $index -le $bytes.Length - 4; $index++) {
                if (
                    $bytes[$index] -eq 13 -and
                    $bytes[$index + 1] -eq 10 -and
                    $bytes[$index + 2] -eq 13 -and
                    $bytes[$index + 3] -eq 10
                ) {
                    $headerEnd = $index + 4
                    break
                }
            }
            if ($memory.Length -gt 81920) {
                throw "The loopback request exceeded the helper's evidence bound."
            }
        } while ($headerEnd -lt 0)

        $headerText = [Text.Encoding]::ASCII.GetString($bytes, 0, $headerEnd)
        $contentLengthMatch = [regex]::Match(
            $headerText,
            '(?im)^Content-Length:\s*(?<length>[0-9]+)\s*$'
        )
        $contentLength = if ($contentLengthMatch.Success) {
            [int]$contentLengthMatch.Groups['length'].Value
        } else {
            0
        }
        if ($contentLength -gt 65536) {
            throw "The loopback request body exceeded Gradial's documented bound."
        }
        while ($memory.Length -lt $headerEnd + $contentLength) {
            $read = $stream.Read($buffer, 0, $buffer.Length)
            if ($read -le 0) {
                throw "The loopback request closed before its body completed."
            }
            $memory.Write($buffer, 0, $read)
        }
        [IO.File]::WriteAllBytes($RequestPath, $memory.ToArray())
    }
    finally {
        $memory.Dispose()
    }

    $response = [Text.Encoding]::ASCII.GetBytes(
        "HTTP/1.1 204 No Content`r`nContent-Length: 0`r`nConnection: close`r`n`r`n"
    )
    $stream.Write($response, 0, $response.Length)
    $stream.Flush()
}
finally {
    if ($stream) {
        $stream.Dispose()
    }
    if ($client) {
        $client.Dispose()
    }
    $listener.Stop()
}
