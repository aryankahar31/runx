# Render with VHS: https://github.com/charmbracelet/vhs
#   vhs demo/runx.tape
# Produces demo/runx.gif. Network access is required for the runtime download.
Output demo/runx.gif

Set Shell "zsh"
Set FontSize 16
Set Width 960
Set Height 540
Set Padding 20
Set Framerate 15
Set TypingSpeed 40ms

Hide
Type "rm -rf /tmp/runx-demo /tmp/runx-demo-home && mkdir -p /tmp/runx-demo /tmp/runx-demo-home/bin && ln -sf /opt/homebrew/opt/runx-launcher/bin/runx /tmp/runx-demo-home/bin/runx && cd /tmp/runx-demo"
Enter
Type `export RUNX_HOME=/tmp/runx-demo-home PATH=/tmp/runx-demo-home/bin:/usr/bin:/bin:/usr/sbin:/sbin PROMPT='$ '`
Enter
Type `echo 20.11.0 > .nvmrc && echo '{"scripts":{"dev":"node --version"}}' > package.json && mkdir -p node_modules && clear`
Enter
Sleep 500ms
Show

Type "cat .nvmrc && cat package.json"
Enter
Sleep 1.5s

Type "runx --version && (which node || echo 'no global node on PATH')"
Enter
Sleep 1.5s

Type "runx dev"
Enter
Sleep 12s

Type "runx dev"
Enter
Sleep 2s

Type "which node || echo 'no global node on PATH'"
Enter
Sleep 2.5s
