import QtQuick
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Ui

BarWidget {
  id: root
  moduleName: "io.github.4m1z.speedy"

  readonly property string executable: Quickshell.env("HOME") + "/.local/bin/speedy"
  property bool statusLoaded: false
  property int today: 0
  property int keysPerMinute: 0
  property bool recorderActive: false
  property int deviceCount: 0

  function compactCount(value) {
    if (value >= 1000000) return (value / 1000000).toFixed(1) + "M"
    if (value >= 1000) return (value / 1000).toFixed(1) + "K"
    return String(value)
  }

  function tooltip() {
    if (!statusLoaded)
      return "Speedy is not installed\nRun the installer from the Speedy repository"
    if (!recorderActive)
      return compactCount(today) + " keys today\nRecorder stopped - right-click to start"
    if (deviceCount === 0)
      return compactCount(today) + " keys today\nNo readable keyboard found; check input-group access"
    return compactCount(today) + " keys today · " + keysPerMinute + " KPM\nClick to open Speedy"
  }

  function refresh() {
    if (!statusProc.running) statusProc.running = true
  }

  function startRecorder() {
    if (!startProc.running) startProc.running = true
  }

  readonly property string appId: "speedy"
  readonly property string windowTitle: "Speedy"

  function openDashboard() {
    Quickshell.execDetached([
      "bash",
      "-lc",
      "title=\"" + windowTitle + "\"; exe=\"" + executable + "\"; addr=$(hyprctl clients -j | jq -r --arg t \"$title\" '.[] | select(.title==$t) | .address' | head -n1); if [ -n \"$addr\" ]; then hyprctl dispatch \"hl.dsp.window.close({ window = \\\"address:$addr\\\" })\" >/dev/null 2>&1 || hyprctl dispatch closewindow address:$addr >/dev/null 2>&1; else xdg-terminal-exec --app-id=\"" + appId + "\" --title=\"$title\" -e \"$exe\" >/dev/null 2>&1 & fi"
    ])
  }

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  Process {
    id: statusProc
    command: [root.executable, "--status"]
    onExited: function(exitCode) {
      if (exitCode !== 0) root.statusLoaded = false
    }
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: {
        try {
          const status = JSON.parse(text || "{}")
          root.today = Number(status.today || 0)
          root.keysPerMinute = Number(status.keysPerMinute || 0)
          root.recorderActive = status.active === true
          root.deviceCount = Number(status.deviceCount || 0)
          root.statusLoaded = true
        } catch (error) {
          root.statusLoaded = false
        }
      }
    }
  }

  Process {
    id: startProc
    command: [root.executable, "--start"]
    onExited: refreshDelay.restart()
    stdout: StdioCollector { waitForEnd: true }
  }

  Timer {
    interval: 2000
    running: true
    repeat: true
    triggeredOnStart: true
    onTriggered: root.refresh()
  }

  Timer {
    id: refreshDelay
    interval: 250
    repeat: false
    onTriggered: root.refresh()
  }

  WidgetButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: root.vertical ? "󰌌" : "󰌌  " + (root.statusLoaded ? root.compactCount(root.today) : "--")
    active: root.statusLoaded && (!root.recorderActive || root.deviceCount === 0)
    tooltipText: root.tooltip()

    onPressed: function(b) {
      if (b === Qt.RightButton) root.startRecorder()
      else if (b === Qt.MiddleButton) root.refresh()
      else root.openDashboard()
    }
  }
}
