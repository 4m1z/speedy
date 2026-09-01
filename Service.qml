import QtQuick
import Quickshell
import Quickshell.Io

Item {
  id: root

  property var shell: null
  readonly property string executable: Quickshell.env("HOME") + "/.local/bin/speedy"

  function startRecorder() {
    if (!startProcess.running) startProcess.running = true
  }

  Process {
    id: startProcess
    command: [root.executable, "--start"]
    onExited: function(exitCode) {
      if (exitCode !== 0) retryTimer.restart()
    }
    stdout: StdioCollector { waitForEnd: true }
  }

  Timer {
    id: retryTimer
    interval: 30000
    repeat: false
    onTriggered: root.startRecorder()
  }

  Component.onCompleted: startRecorder()
}
