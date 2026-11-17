# Render with VHS: https://github.com/charmbracelet/vhs
#   vhs demo/runx.tape
# Produces demo/runx.gif. Network access is required for the runtime download.
Output demo/runx.gif

Set Shell "zsh"
Set FontSize 18
Set Width 1100
Set Height 650
Set Padding 24
Set TypingSpeed 55ms

Hide
Type "rm -rf /tmp/runx-demo && mkdir -p /tmp/runx-demo && cd /tmp/runx-demo"
Enter
Hide

Type "echo 20.11.0 > .nvmrc"
Enter
Type "echo '{\"scripts\":{\"dev\":\"node --version\"}}' > package.json"
Enter

Type "runx init"
Enter
Sleep 2s

Type "runx doctor"
Enter
Sleep 2s

Type "runx dev"
Enter
Sleep 8s

Type "runx dev"
Enter
Sleep 2s
