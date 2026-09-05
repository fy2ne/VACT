# 🎬 VACT Agent Runner (`vact-agent`)
### Powered 100% by Google Cloud Vertex AI & Google Gemini

[![License: Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg?style=flat-square)](https://opensource.org/licenses/Apache-2.0)
[![TypeScript](https://img.shields.io/badge/TypeScript-Strict-blue?style=flat-square)](https://www.typescriptlang.org/)
[![Google Cloud](https://img.shields.io/badge/Google%20Cloud-Vertex%20AI-4285F4?logo=googlecloud&logoColor=white)](https://cloud.google.com/vertex-ai)
[![Gemini](https://img.shields.io/badge/Google-Gemini%203.7%20%2F%202.5-8E75B2?logo=googlegemini&logoColor=white)](https://ai.google.dev/)

An autonomous AI desktop agent engine built exclusively on **Google Cloud Vertex AI** and **Google AI Studio** with the **Google Gemini** model family (Gemini 3.7 Flash/Pro/Thinking, 2.5 Flash/Pro, 2.0 Flash, 1.5 Flash).

> [!TIP]
> For a full visual setup guide with architecture flowcharts, see [**`GOOGLE_AI_SETUP_GUIDE.md`**](../../GOOGLE_AI_SETUP_GUIDE.md).

---

## ⚡ Highlights

- **Zero Screenshots & Zero Vision Latency:** Operates on lightweight vector scene DAGs (< 2 KB per frame) directly in Gemini context.
- **Direct GPU Photometric Color Grounding:** Dominant RGB, hex code (`#FF3B30`), and semantic colors (`red`, `blue`, `green`, `orange`, `yellow`) extracted directly from the GPU frame buffer in $<1\,\mu\text{s}$.
- **Interactive Multi-Model CLI Wizard:** Color-coded quota profiles, live Google API scanner (`[S]`), custom model input (`[C]`), and cinema presets.
- **100% Google Cloud Native:** Pure Gemini 3.7 / 2.5 / 2.0 reasoning loop.

---

## 🚀 Quick Setup & Authentication

### 1. Install Dependencies
```bash
cd apps/agent-runner
npm install
```

### 2. Configure Your Preferred Google Backend

#### Option A: Google AI Studio (Fastest)
In `.env`:
```env
GEMINI_API_KEY=AIzaSyYourSecretKeyHere
GOOGLE_BACKEND=ai-studio
GEMINI_MODEL=gemini-2.5-flash
```

#### Option B: Google Cloud Vertex AI (Enterprise)
Authenticate using Google Cloud CLI:
```bash
gcloud auth application-default login
gcloud config set project YOUR_GOOGLE_CLOUD_PROJECT_ID
```
In `.env`:
```env
GOOGLE_CLOUD_PROJECT=your-google-cloud-project-id
GOOGLE_CLOUD_LOCATION=us-central1
GOOGLE_BACKEND=vertex
GEMINI_MODEL=gemini-2.5-flash
```

---

## 🎮 Interactive CLI Wizard

Simply run:
```bash
npm run dev
```

The interactive wizard will guide you to:
1. Select Google Backend (`Google AI Studio` or `Google Cloud Vertex AI`).
2. Choose from the full fleet of Gemini models (with color-coded RPM/TPM limits), type a custom model (`[C]`), or scan live models (`[S]`).
3. Select a preset mission or enter a custom prompt.

---

## 🧪 Testing & Validation

Run the test suite:
```bash
npm test
```

Typecheck:
```bash
npm run typecheck
```

---

## 👥 Authors

- **fy2ne ([@fy2ne](https://github.com/fy2ne))**: Core Architecture, DirectX GPU Compute & Rust Daemon (`<hello@fy2ne.me>`)

## 📄 License

Licensed under the **Apache License 2.0** — see the [`LICENSE`](../../LICENSE) file for details.
