$ErrorActionPreference = "Continue"
$out = "E:\Code\ai\md\audit\_tr2d_word.txt"
"" | Set-Content -Path $out -Encoding UTF8
function W($s) { Add-Content -Path $out -Value $s -Encoding UTF8 }
$src = "E:\Code\ai\md\audit\tr2-export.docx"
$w = New-Object -ComObject Word.Application
$w.Visible = $false
$w.DisplayAlerts = 0
$doc = $w.Documents.Open($src, $false, $true, $false)
W ("pages=" + $doc.ComputeStatistics(2) + " words=" + $doc.ComputeStatistics(0))
W ""
W "=== paragraph style/size/bold ==="
for ($i = 1; $i -le $doc.Paragraphs.Count; $i++) {
  $p = $doc.Paragraphs.Item($i)
  $t = ($p.Range.Text -replace "[\r\n\a]", "")
  if ($t.Length -gt 40) { $t = $t.Substring(0,40) }
  $f = $p.Range.Font
  W ("[" + $i + "] style=" + $p.Style.NameLocal + " size=" + $f.Size + " bold=" + $f.Bold + " outline=" + $p.OutlineLevel + " | " + $t)
}
W ""
W "=== style defs (G2) ==="
$s1 = $doc.Styles.Item(-2)
W ("Heading1: size=" + $s1.Font.Size + " bold=" + $s1.Font.Bold + " font=" + $s1.Font.Name)
$s2 = $doc.Styles.Item(-1)
W ("Normal: size=" + $s2.Font.Size + " font=" + $s2.Font.Name)
W ""
W "=== page setup (G2 geometry) ==="
$ps = $doc.PageSetup
W ("margins cm T" + [math]::Round($ps.TopMargin/28.35,2) + " B" + [math]::Round($ps.BottomMargin/28.35,2) + " L" + [math]::Round($ps.LeftMargin/28.35,2) + " R" + [math]::Round($ps.RightMargin/28.35,2))
W ""
W "=== bold loss check (P0) ==="
$found = 0
for ($i = 1; $i -le $doc.Paragraphs.Count; $i++) {
  $t = ($doc.Paragraphs.Item($i).Range.Text -replace "[\r\n\a]", "")
  if ($t.Contains("**")) { W ("ASTERISK-RESIDUE [" + $i + "]: " + $t); $found++ }
}
if ($found -eq 0) { W "no-asterisk-residue" }
W ""
W "=== TOC ==="
W ("TOC=" + $doc.TablesOfContents.Count + " fields=" + $doc.Fields.Count)
$doc.Close([ref]0)
$w.Quit()