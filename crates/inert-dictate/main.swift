// inert-dictate: push-to-talk speech recognition for Inert Browser.
//
// The browser starts this helper when Option+Space goes down and writes
// "stop" to its stdin when the key comes up. While it runs it prints
//   P <text so far>      (partial results)
//   F <final text>       (once, then exits)
//   E <reason>           (on failure, then exits)
// Recognition uses Apple's Speech framework, on this Mac when the language
// supports it (no audio leaves the machine in that case).

import AVFoundation
import Foundation
import Speech

setvbuf(stdout, nil, _IOLBF, 0)

func fail(_ why: String) -> Never {
    print("E \(why)")
    exit(1)
}

let lang = CommandLine.arguments.dropFirst().first ?? Locale.preferredLanguages.first ?? "en-US"
guard let recognizer = SFSpeechRecognizer(locale: Locale(identifier: lang)) ?? SFSpeechRecognizer() else {
    fail("Speech recognition isn't available for \(lang)")
}

let engine = AVAudioEngine()
let request = SFSpeechAudioBufferRecognitionRequest()
request.shouldReportPartialResults = true
if recognizer.supportsOnDeviceRecognition {
    request.requiresOnDeviceRecognition = true
}
var last = ""
var stopping = false

func finish() {
    print("F \(last)")
    exit(0)
}

func start() {
    guard recognizer.isAvailable else { fail("Speech recognition is not available right now") }
    let input = engine.inputNode
    let format = input.outputFormat(forBus: 0)
    guard format.sampleRate > 0 else { fail("No microphone") }
    input.installTap(onBus: 0, bufferSize: 1024, format: format) { buffer, _ in
        request.append(buffer)
    }
    _ = recognizer.recognitionTask(with: request) { result, error in
        if let r = result {
            last = r.bestTranscription.formattedString.replacingOccurrences(of: "\n", with: " ")
            if r.isFinal {
                finish()
            }
            print("P \(last)")
        }
        if error != nil {
            // After "stop" an error just means there was nothing more to hear.
            if stopping || !last.isEmpty { finish() }
            fail("Didn't catch that")
        }
    }
    engine.prepare()
    do {
        try engine.start()
    } catch {
        fail("Microphone: \(error.localizedDescription)")
    }
    print("R")
}

SFSpeechRecognizer.requestAuthorization { status in
    guard status == .authorized else { fail("Allow Speech Recognition for Inert in System Settings → Privacy & Security") }
    AVCaptureDevice.requestAccess(for: .audio) { ok in
        guard ok else { fail("Allow the Microphone for Inert in System Settings → Privacy & Security") }
        DispatchQueue.main.async { start() }
    }
}

// "stop" (or the browser going away) ends the recording; the recognizer
// then delivers its final result.
DispatchQueue.global().async {
    while let line = readLine() {
        if line.hasPrefix("stop") { break }
    }
    DispatchQueue.main.async {
        stopping = true
        engine.stop()
        engine.inputNode.removeTap(onBus: 0)
        request.endAudio()
        // If the recognizer has nothing more to say, don't hang.
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) { finish() }
    }
}

RunLoop.main.run()
