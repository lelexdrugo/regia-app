# The moviola as Windows' UI Automation has it (#490), for prove/uia.mjs: what Narrator and NVDA read, from the tree of
# the window of one process. Windows PowerShell 5.1, by path:
#   powershell.exe -NoProfile -ExecutionPolicy Bypass -File prove/uia.ps1 -ProcessId <id> -Stage "<the stage's name>"
# It prints one JSON object: the window; the stage's list, its items and groups, and the item that holds the keyboard's
# focus or is selected, with the element it sits in; the items of every other list whose name starts with -Lists, or
# of every other list with -Lists "*".
param(
  [Parameter(Mandatory = $true)][int]$ProcessId,
  [Parameter(Mandatory = $true)][string]$Stage,
  [string]$Lists = ""
)
$ErrorActionPreference = "Stop"
trap {
  [Console]::Error.WriteLine("uia.ps1 line $($_.InvocationInfo.ScriptLineNumber): $_")
  exit 1
}
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$A = [System.Windows.Automation.AutomationElement]
$Scope = [System.Windows.Automation.TreeScope]
$Type = [System.Windows.Automation.ControlType]
# The raw view: the managed client's FindAll does not cross from the host's window into the browser's fragment, where
# the page's tree is; a walk of the raw view does.
$Walker = [System.Windows.Automation.TreeWalker]::RawViewWalker

function Describe($element) {
  if ($null -eq $element) { return $null }
  $current = $element.Current
  $selected = $null
  $pattern = $null
  if ($element.TryGetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern, [ref]$pattern)) {
    $selected = $pattern.Current.IsSelected
  }
  return [ordered]@{
    type = $current.ControlType.ProgrammaticName -replace "^ControlType\.", ""
    name = $current.Name
    focus = $current.HasKeyboardFocus
    selected = $selected
  }
}

# Every element under `root` of one of `types`, in the tree's order, up to `limit` elements walked.
function Find-Under($root, $types, [int]$limit = 8000) {
  $found = New-Object System.Collections.ArrayList
  $stack = New-Object System.Collections.Stack
  $stack.Push($root)
  $walked = 0
  while ($stack.Count -gt 0 -and $walked -lt $limit) {
    $element = $stack.Pop()
    $walked++
    if ($types -contains $element.Current.ControlType) { [void]$found.Add($element) }
    $children = New-Object System.Collections.ArrayList
    $child = $Walker.GetFirstChild($element)
    while ($null -ne $child) { [void]$children.Add($child); $child = $Walker.GetNextSibling($child) }
    for ($i = $children.Count - 1; $i -ge 0; $i--) { $stack.Push($children[$i]) }
  }
  return ,$found
}

$byProcess = New-Object System.Windows.Automation.PropertyCondition($A::ProcessIdProperty, $ProcessId)
$window = $null
for ($try = 0; $try -lt 50 -and $null -eq $window; $try++) {
  $window = $A::RootElement.FindFirst($Scope::Children, $byProcess)
  if ($null -eq $window) { Start-Sleep -Milliseconds 200 }
}
if ($null -eq $window) { throw "no window of process $ProcessId" }

$stageList = $null
$others = @()
foreach ($list in (Find-Under $window @($Type::List))) {
  [string]$name = $list.Current.Name
  if ($name -eq $Stage) { $stageList = $list }
  elseif ($Lists -eq "*" -or ($Lists -ne "" -and $name.StartsWith($Lists))) { $others += $list }
}

$out = [ordered]@{ window = $window.Current.Name; stage = $null; lists = @() }
if ($null -ne $stageList) {
  $under = Find-Under $stageList @($Type::ListItem, $Type::Group)
  $items = @($under | Where-Object { $_.Current.ControlType -eq $Type::ListItem })
  $groups = @($under | Where-Object { $_.Current.ControlType -eq $Type::Group })
  $chosen = $null
  foreach ($item in $items) {
    $d = Describe $item
    if ($d.focus -or $d.selected) { $chosen = $item; break }
  }
  $out.stage = [ordered]@{
    list = Describe $stageList
    items = $items.Count
    groups = $groups.Count
    chosen = Describe $chosen
    within = if ($null -ne $chosen) { Describe ($Walker.GetParent($chosen)) } else { $null }
  }
}
foreach ($list in $others) {
  $names = @()
  foreach ($item in (Find-Under $list @($Type::ListItem) 400)) { $names += $item.Current.Name }
  $out.lists += [ordered]@{ name = $list.Current.Name; items = $names }
}
$out | ConvertTo-Json -Depth 6 -Compress
