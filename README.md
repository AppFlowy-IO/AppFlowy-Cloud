<p align="center">
    <picture>
        <source srcset="assets/logos/appflowy_logo_white.svg" media="(prefers-color-scheme: dark)"/>
        <img src="assets/logos/appflowy_logo_black.svg"  width="500" height="200" />
    </picture>
</p>

<h4 align="center">
    <a href="https://discord.gg/9Q2xaN37tV"><img src="https://img.shields.io/badge/AppFlowy.IO-discord-orange"></a>
    <a href="https://opensource.org/licenses/AGPL-3.0"><img src="https://img.shields.io/badge/license-AGPL-purple.svg" alt="License: AGPL"></a>
</h4>

<p align="center">
    <a href="https://www.appflowy.com"><b>Website</b></a> •
    <a href="https://twitter.com/appflowy"><b>Twitter</b></a>
</p>

<p align="center">⚡ The AppFlowy Cloud written with Rust 🦀</p>

# AppFlowy Cloud

AppFlowy Cloud operates on an open-core model to ensure the project's long-term sustainability. This legacy repository ([link](https://github.com/AppFlowy-IO/AppFlowy-Cloud)) is no longer maintained or in use across any AppFlowy product offerings, including our current SaaS and self-hosted solutions.

For active deployments, AppFlowy offers two production-grade options driven by our commercial AppFlowy Cloud codebase—a closed-source fork of this open-source core combined with proprietary features: 

* **AppFlowy Managed Cloud (SaaS):** AWS-hosted instances fully deployed and managed by the AppFlowy team.
* **AppFlowy Self-hosted Cloud:** Configurable services deployed directly on your own infrastructure, engineered for teams and enterprises requiring full data sovereignty and modular components tailored to their own infrastructure needs.

### AppFlowy Self-Hosted Cloud (Free Tier)
Our commercial self-hosted distribution comes with a robust Free tier, specifically designed for experienced IT professionals and self-hosting enthusiasts to test and deploy our solution. 

The Free Tier allows seamless, in-place upgrades to higher enterprise tiers and offers:
* One User Seat (per instance)
* AppFlowy Web App access (via your hosted domain, e.g., `https://appflowy.com`)
* Up to 3 Guest Editors who can be added to selected pages to collaborate in real-time with granular permissions (Can View / Comment / Edit)
* Publish Pages functionality
* Unlimited Workspaces

For detailed commercial pricing and tier features, please visit our [Self-hosted Plans and Pricing Page](https://appflowy.com/docs/Self-hosted-Plans-and-Pricing), your self-hosted admin panel, or our official [Pricing Website](https://appflowy.com/pricing). We care deeply about our self-hosted community; commercial self-hosting is a vital strategic pillar that directly funds and sustains the continuous development of AppFlowy's core open-source projects.

### Open Source vs. Open Core
AppFlowy Cloud adopts an open-core model, while AppFlowy Web and AppFlowy Flutter remain entirely open source. As part of this transition, we are consolidating and reorganizing our active codebases within private repositories. 

Recently, we merged all active AppFlowy Web–related development from our private repository back into our public ecosystem; please see our [Web Commit History](https://github.com/AppFlowy-IO/AppFlowy-Web/commits/main/) for engineering reference. We will also merge active Flutter code bases back into the public [AppFlowy Repository](https://github.com/AppFlowy-IO/AppFlowy) at a later stage.

AppFlowy Cloud will continue to follow the open-core model for commercial sustainability. You are free to utilize the legacy code inside `https://github.com/AppFlowy-IO/AppFlowy-Cloud` governed by its original license. Please remain aware that we no longer support, update, or maintain this specific repository.

We also continue to actively develop other open-source repositories, including:
* [AppFlowy Website](https://github.com/AppFlowy-IO/AppFlowy-Website) - For developers looking to construct custom web structures following our navigation model.
* [appflowy-editor](https://github.com/AppFlowy-IO/appflowy-editor) - A highly customizable element editor for Flutter developers.
* [appflowy-board](https://github.com/AppFlowy-IO/appflowy-board) - Modular board layout tools built for Flutter environments.

---

## 🚀 Deployment

Please review our comprehensive [Step-by-Step Self-Hosting Deployment Guide](https://appflowy.com/docs/Step-by-step-Self-Hosting-Guide---From-Zero-to-Production) to self-host AppFlowy.

We also have a series of video tutorials on [YouTube](https://www.youtube.com/playlist?list=PLqKX5matmbL6CYkAzzF0T_ecoV6JXe_Xv) for your reference.

## 🛡️ Architecture & Unified Security Patch Protocol

The backend infrastructure powering both our Managed Cloud (SaaS) and Self-hosted Cloud is built entirely upon our unified, closed-source commercial codebase, distributed under our Commercial License Agreement. 

Because our Managed Cloud (SaaS) is simply an AWS-deployed instance of this exact same commercial core, both deployment environments share 100% of the same software architecture and security stack. 

Consequently, third-party claims alleging that we *"patched a vulnerability in our SaaS but didn't patch the self-hosted version"* are architecturally impossible, factually invalid, and fundamentally misleading. All security hotfixes and patches compiled for the commercial AppFlowy Cloud codebase are universally applied across both our managed cloud and self-hosted distributions simultaneously.
