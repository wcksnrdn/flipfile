# Third-Party Notices

flipfile is built on a number of open-source components and AI models. Their licenses
are reproduced below and, where required, inside the distributed application bundle.

---

## AI Models

### Real-ESRGAN (upscale.mlmodelc / upscale.onnx, upscale-text.mlmodelc / upscale-text.onnx, 70% of upscale-photo.mlmodelc / upscale-photo.onnx)

Authors: Xintao Wang et al.  
Source: https://github.com/xinntao/Real-ESRGAN  
License: **BSD 3-Clause**

The weights were converted to Core ML and ONNX; the compact upscaler is an even blend of
`realesr-general-x4v3` and `realesr-general-wdn-x4v3`; RealESRNet x4plus
(`upscale-text`, Upscale Text's shapes) is stored in float16.

```
BSD 3-Clause License

Copyright (c) 2021, Xintao Wang
All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this
   list of conditions and the following disclaimer.

2. Redistributions in binary form must reproduce the above copyright notice,
   this list of conditions and the following disclaimer in the documentation
   and/or other materials provided with the distribution.

3. Neither the name of the copyright holder nor the names of its
   contributors may be used to endorse or promote products derived from
   this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

---

### RestoreFormer++ (faces.mlmodelc / faces.onnx)

Authors: Zhouxia Wang, Jiawei Zhang, Tianshui Chen, Wenping Wang, Ping Luo  
Source: https://github.com/wzhouxiff/RestoreFormerPlusPlus  
Paper: "RestoreFormer++: Towards Real-World Blind Face Restoration from Undegraded
Key-Value Pairs", IEEE TPAMI 2023  
License: **Apache 2.0**

The weights were converted to Core ML and ONNX with 8-bit weight quantisation.

```
                                 Apache License
                           Version 2.0, January 2004
                        http://www.apache.org/licenses/

   TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION

   1. Definitions.

      "License" shall mean the terms and conditions for use, reproduction,
      and distribution as defined by Sections 1 through 9 of this document.

      "Licensor" shall mean the copyright owner or entity authorized by
      the copyright owner that is granting the License.

      "Legal Entity" shall mean the union of the acting entity and all
      other entities that control, are controlled by, or are under common
      control with that entity. For the purposes of this definition,
      "control" means (i) the power, direct or indirect, to cause the
      direction or management of such entity, whether by contract or
      otherwise, or (ii) ownership of fifty percent (50%) or more of the
      outstanding shares, or (iii) beneficial ownership of such entity.

      "You" (or "Your") shall mean an individual or Legal Entity
      exercising permissions granted by this License.

      "Source form" shall mean the preferred form for making modifications,
      including but not limited to software source code, documentation
      source, and configuration files.

      "Object form" shall mean any form resulting from mechanical
      transformation or translation of a Source form, including but
      not limited to compiled object code, generated documentation,
      and conversions to other media types.

      "Work" shall mean the work of authorship, whether in Source or
      Object form, made available under the License, as indicated by a
      copyright notice that is included in or attached to the work
      (an example is provided in the Appendix below).

      "Derivative Works" shall mean any work, whether in Source or Object
      form, that is based on (or derived from) the Work and for which the
      editorial revisions, annotations, elaborations, or other modifications
      represent, as a whole, an original work of authorship. For the purposes
      of this License, Derivative Works shall not include works that remain
      separable from, or merely link (or bind by name) to the interfaces of,
      the Work and Derivative Works thereof.

      "Contribution" shall mean any work of authorship, including
      the original version of the Work and any modifications or additions
      to that Work or Derivative Works thereof, that is intentionally
      submitted to Licensor for inclusion in the Work by the copyright owner
      or by an individual or Legal Entity authorized to submit on behalf of
      the copyright owner. For the purposes of this definition, "submitted"
      means any form of electronic, verbal, or written communication sent
      to the Licensor or its representatives, including but not limited to
      communication on electronic mailing lists, source code control systems,
      and issue tracking systems that are managed by, or on behalf of, the
      Licensor for the purpose of discussing and improving the Work, but
      excluding communication that is conspicuously marked or otherwise
      designated in writing by the copyright owner as "Not a Contribution."

      "Contributor" shall mean Licensor and any individual or Legal Entity
      on behalf of whom a Contribution has been received by Licensor and
      subsequently incorporated within the Work.

   2. Grant of Copyright License. Subject to the terms and conditions of
      this License, each Contributor hereby grants to You a perpetual,
      worldwide, non-exclusive, no-charge, royalty-free, irrevocable
      copyright license to reproduce, prepare Derivative Works of,
      publicly display, publicly perform, sublicense, and distribute the
      Work and such Derivative Works in Source or Object form.

   3. Grant of Patent License. Subject to the terms and conditions of
      this License, each Contributor hereby grants to You a perpetual,
      worldwide, non-exclusive, no-charge, royalty-free, irrevocable
      (except as stated in this section) patent license to make, have made,
      use, offer to sell, sell, import, and otherwise transfer the Work,
      where such license applies only to those patent claims licensable
      by their Contribution(s) alone or by combination of their Contribution(s)
      with the Work to which such Contribution(s) was submitted. If You
      institute patent litigation against any entity (including a cross-claim
      or counterclaim in a lawsuit) alleging that the Work or a Contribution
      incorporated within the Work constitutes direct or contributory patent
      infringement, then any patent licenses granted to You under this License
      for that Work shall terminate as of the date such litigation is filed.

   4. Redistribution. You may reproduce and distribute copies of the Work or
      Derivative Works thereof in any medium, with or without modifications,
      and in Source or Object form, provided that You meet the following
      conditions:

      (a) You must give any other recipients of the Work or Derivative Works
          a copy of this License; and

      (b) You must cause any modified files to carry prominent notices stating
          that You changed the files; and

      (c) You must retain, in the Source form of any Derivative Works that You
          distribute, all copyright, patent, trademark, and attribution notices
          from the Source form of the Work, excluding those notices that do not
          pertain to any part of the Derivative Works; and

      (d) If the Work includes a "NOTICE" text file, You must include a
          readable copy of the attribution notices contained in that NOTICE
          file, in at least one of the following places: within a NOTICE text
          file distributed as part of the Derivative Works; within the Source
          form or documentation, if provided along with the Derivative Works;
          or, within a display generated by the Derivative Works, if and
          wherever such third-party notices normally appear. The contents of
          the NOTICE file are for informational purposes only and do not modify
          the License. You may add Your own attribution notices within
          Derivative Works that You distribute, alongside or as an addendum to
          the NOTICE text from the Work, provided that such additional
          attribution notices cannot be construed as modifying the License.

   5. Submission of Contributions. Unless You explicitly state otherwise, any
      Contribution intentionally submitted for inclusion in the Work by You to
      the Licensor shall be under the terms and conditions of this License,
      without any additional terms or conditions. Notwithstanding the above,
      nothing herein shall supersede or modify the terms of any separate
      license agreement you may have executed with Licensor regarding such
      Contributions.

   6. Trademarks. This License does not grant permission to use the trade
      names, trademarks, service marks, or product names of the Licensor,
      except as required for reasonable and customary use in describing the
      origin of the Work and reproducing the content of the NOTICE file.

   7. Disclaimer of Warranty. Unless required by applicable law or agreed to
      in writing, Licensor provides the Work (and each Contributor provides
      its Contributions) on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS
      OF ANY KIND, either express or implied, including, without limitation,
      any warranties or conditions of TITLE, NON-INFRINGEMENT,
      MERCHANTABILITY, or FITNESS FOR A PARTICULAR PURPOSE. You are solely
      responsible for determining the appropriateness of using or
      redistributing the Work and assume any risks associated with Your
      exercise of permissions under this License.

   8. Limitation of Liability. In no event and under no legal theory, whether
      in tort (including negligence), contract, or otherwise, unless required
      by applicable law (such as deliberate and grossly negligent acts) or
      agreed to in writing, shall any Contributor be liable to You for
      damages, including any direct, indirect, special, incidental, or
      consequential damages of any character arising as a result of this
      License or out of the use or inability to use the Work (even if such
      Contributor has been advised of the possibility of such damages).

   9. Accepting Warranty or Additional Liability. While redistributing the
      Work or Derivative Works thereof, You may choose to offer, and charge a
      fee for, acceptance of support, warranty, indemnity, or other liability
      obligations and/or rights consistent with this License. However, in
      accepting such obligations, You may act only on Your own behalf and on
      Your sole responsibility, not on behalf of any other Contributor, and
      only if You agree to indemnify, defend, and hold each Contributor
      harmless for any liability incurred by, or claims asserted against, such
      Contributor by reason of your accepting any such warranty or additional
      liability.

   END OF TERMS AND CONDITIONS
```

---

### 4xFaceUpSharpLDAT — DAT face upscaler (faces-upscale.mlmodelc / faces-upscale.onnx)

Author: Philip Hofmann (Phhofm)  
Source: https://github.com/Phhofm/models/releases/tag/4xFaceUpDAT_Series  
Network: DAT — Z. Chen et al., "Dual Aggregation Transformer for Image Super-Resolution", ICCV 2023  
License: **Creative Commons Attribution 4.0 International (CC BY 4.0)**  
https://creativecommons.org/licenses/by/4.0/

Copyright (c) 2023 Philip Hofmann (Phhofm)

Changes made for flipfile: the released weights (`4xFaceUpSharpLDAT.safetensors`) were
converted to Core ML (`faces-upscale.mlmodelc`) and ONNX (`faces-upscale.onnx`) for
128 px tiles, with the weights stored as 8-bit integers; the network (DAT) was
reimplemented for this in `scripts/upscale-model/dat.py`.

---

### YuNet — face detector (yunet.onnx, Windows and Linux only)

Authors: Shiqi Yu et al.  
Source: https://github.com/opencv/opencv_zoo/tree/main/models/face_detection_yunet  
Paper: W. Wu, H. Peng, S. Yu, "YuNet: A Tiny Millisecond-level Face Detector",
Machine Intelligence Research 2023  
License: **MIT**

The model file `face_detection_yunet_2026may.onnx` is used unchanged.

```
MIT License

Copyright (c) 2020 Shiqi Yu <shiqi.yu@gmail.com>

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

---

### 4xNomos8kSC — 30% of Upscale Photo's detailed upscaler (upscale-photo.mlmodelc / upscale-photo.onnx)

Author: Philip Hofmann (Phhofm)  
Source: https://github.com/Phhofm/models/releases/tag/4xNomos8kSC  
Network: RRDBNet (Real-ESRGAN, X. Wang et al., ICCVW 2021), trained further on the
Nomos8k dataset with JPEG compression and slight blur  
License: **Creative Commons Attribution 4.0 International (CC BY 4.0)**  
https://creativecommons.org/licenses/by/4.0/

Copyright (c) 2023 Philip Hofmann (Phhofm)

Changes made for flipfile: the released weights (`4xNomos8kSC.safetensors`) were blended
30% with 70% of Real-ESRGAN x4plus's weights (network interpolation), and the result
converted to Core ML (`upscale-photo.mlmodelc`) and ONNX (`upscale-photo.onnx`) for
256 px tiles, with the weights stored in float16.

---

### FBCNN — JPEG artefact removal for Upscale Text (dejpeg.mlmodelc / dejpeg.onnx)

Authors: Jiaxi Jiang, Kai Zhang, Radu Timofte  
Source: https://github.com/jiaxi-jiang/FBCNN  
Paper: "Towards Flexible Blind JPEG Artifacts Removal", ICCV 2021  
License: **Apache 2.0** (full text in `LICENSE-FBCNN.txt`)

The published `fbcnn_color` weights were converted to Core ML and ONNX for 256 px tiles
and stored in float16; no source code of FBCNN is included in the app.

---

### DDColor — colours black-and-white photos (colorize.mlmodelc / colorize.onnx)

Authors: Xiaoyang Kang, Tao Yang, Wenqi Ouyang, Peiran Ren, Lingzhi Li, Xuansong Xie  
Source: https://github.com/piddnad/DDColor  
Paper: "DDColor: Towards Photo-Realistic Image Colorization via Dual Decoders", ICCV 2023  
License: **Apache 2.0** (full text in `LICENSE-DDColor.txt`)

The published `ddcolor_paper_tiny` weights were converted to Core ML and ONNX for a
512 x 512 input and stored in float16; no source code of DDColor is included in the app.

---

### Fonts for Upscale Text (models/fonts)

Upscale Text types words again in the open font closest to the picture's. All are used
unchanged, except Gelasio, Noto Sans and Open Sans, whose Regular and Bold were made
from the published variable fonts (fontTools instancer); none of them declares a
Reserved Font Name. Each licence is in `models/fonts/LICENSE-<family>.txt`.

| Fonts | Authors | License |
|---|---|---|
| Liberation Sans, Serif, Mono 2.1.5 | Red Hat, Inc. (digitized data Google) | SIL Open Font License 1.1 |
| Gelasio | The Gelasio Project Authors | SIL Open Font License 1.1 |
| Carlito | The Carlito Project Authors | SIL Open Font License 1.1 |
| Caladea | The Caladea Project Authors | SIL Open Font License 1.1 |
| DejaVu Sans 2.37 | DejaVu fonts team, Bitstream | Bitstream Vera Fonts licence (permissive) |
| Inter 4.1 | The Inter Project Authors | SIL Open Font License 1.1 |
| Roboto 3 | The Roboto Project Authors | SIL Open Font License 1.1 |
| Noto Sans | The Noto Project Authors | SIL Open Font License 1.1 |
| Open Sans | The Open Sans Project Authors | SIL Open Font License 1.1 |

---

### Word list for Upscale Text (words.txt)

Source: Leipzig Corpora Collection, corpora `ind_mixed_2013_1M` and `eng_news_2024_1M`  
© Universität Leipzig / Sächsische Akademie der Wissenschaften / InfAI  
License: **Creative Commons Attribution (CC BY)**, https://wortschatz.uni-leipzig.de/en/usage

Only words made of letters and used at least three times were kept, lowercased, without
their counts; macOS also asks its own spelling dictionaries.

---

### Whisper (whisper-small.bin)

Authors: Alec Radford, Jong Wook Kim, Tao Xu, Greg Brockman, Christine McLeavey,
Ilya Sutskever (OpenAI)  
Source: https://github.com/openai/whisper  
GGML file: https://huggingface.co/ggerganov/whisper.cpp (`ggml-small-q5_1.bin`, unchanged)  
License: **MIT**

```
MIT License

Copyright (c) 2022 OpenAI

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

---

### Silero VAD (whisper-vad.bin)

Authors: Silero Team  
Source: https://github.com/snakers4/silero-vad  
GGML file: https://huggingface.co/ggml-org/whisper-vad (`ggml-silero-v6.2.0.bin`, unchanged)  
License: **MIT**

```
MIT License

Copyright (c) 2020-present Silero Team

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

---

### whisper.cpp / ggml

Authors: The ggml authors  
Source: https://github.com/ggerganov/whisper.cpp  
License: **MIT**

Compiled into flipfile through the `whisper-rs` crate (public domain).

```
MIT License

Copyright (c) 2023-2024 The ggml authors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

---

## Rust Crates

### Symphonia

Authors: Philip Deljanov and contributors  
Source: https://github.com/pdeljanov/Symphonia  
License: **Mozilla Public License 2.0 (MPL-2.0)**  
https://www.mozilla.org/en-US/MPL/2.0/

Used unmodified for audio decoding (AAC, ALAC, FLAC, MP3, PCM, Vorbis, ADPCM) across
isomp4, mkv, ogg, aiff, wav, caf, and other containers.

> MPL-2.0 requires that any modifications to MPL-licensed files be made available under
> the same license. flipfile uses Symphonia unmodified as a crate dependency; no
> Symphonia source files are included in this repository.

---

### opus1

Source: https://crates.io/crates/opus1  
License: **BSD 3-Clause**

Used unmodified for Opus audio decoding (WhatsApp voice notes, OGG/WebM).

---

### ONNX Runtime (Windows and Linux)

Authors: Microsoft  
Source: https://www.nuget.org/packages/Microsoft.ML.OnnxRuntime (Windows: Microsoft.ML.OnnxRuntime.DirectML)  
Version: 1.24.4  
License: **MIT**

Used unmodified from Microsoft's official NuGet packages, verified by SHA-256
(`scripts/fetch-onnxruntime.mjs`). License and third-party notices ship inside the
application bundle alongside the library (`onnxruntime/LICENSE-onnxruntime.txt` and
`ThirdPartyNotices-onnxruntime.txt`).

---

### DirectML (Windows)

Authors: Microsoft  
License: See `onnxruntime/LICENSE-DirectML.txt` in the application bundle.

Used unmodified from Microsoft's official release as part of the ONNX Runtime
distribution for GPU-accelerated AI upscaling on Windows.

---

## Full license texts for AI models

The full license texts for every AI model (downloaded as packs from the Ring Workshop) are
also included in the application bundle at `Resources/models/` (macOS) or alongside the
executable (Windows/Linux):

- `LICENSE-Real-ESRGAN.txt` — BSD 3-Clause
- `LICENSE-RestoreFormerPlusPlus.txt` — Apache 2.0
- `LICENSE-FaceUp.txt` — CC BY 4.0
- `LICENSE-Nomos8kSC.txt` — CC BY 4.0
- `LICENSE-FBCNN.txt` — Apache 2.0
- `LICENSE-DDColor.txt` — Apache 2.0
- `LICENSE-words.txt` — CC BY
- `fonts/LICENSE-*.txt` — SIL Open Font License 1.1, Bitstream Vera
- `LICENSE-YuNet.txt` — MIT
- `LICENSE-Whisper.txt` — MIT
- `LICENSE-Silero-VAD.txt` — MIT
- `LICENSE-whisper.cpp.txt` — MIT
