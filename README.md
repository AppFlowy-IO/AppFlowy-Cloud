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

AppFlowy Cloud is adopting an open-core model to ensure the project's long-term sustainability.

AppFlowy offers two deployment options:
- AppFlowy Managed Cloud: AWS-hosted instances fully deployed and managed by the AppFlowy team
- AppFlowy Self-hosted Cloud: Configurable services you can deploy on your own infrastructure

The codebase behind these two setups is a closed-source fork of this open-source codebase: https://github.com/AppFlowy-IO/AppFlowy-Cloud, combined with our proprietary code. 
The commercial fork is distributed solely under our commercial license ([link](https://github.com/AppFlowy-IO/AppFlowy-SelfHost-Commercial/blob/main/SELF_HOST_LICENSE_AGREEMENT.md)). 


**AppFlowy Self-hosted Cloud is designed for teams and enterprises that want data control and modular, configurable components tailored to their own infrastructure.** 
It comes with a Free tier suitable for **experienced IT professionals to test out our self-hosted solution** and allows seamless upgrades to higher tiers.

The Free tier offers:
- One user seat (per instance)
- AppFlowy Web App (your hosted appflowy.com/app)
- Up to 3 guest editors who can be added to your selected AppFlowy pages and collaborate with you in real time
- Publish pages
- Unlimited workspaces

You can find the pricing details by visiting [this page](https://appflowy.com/docs/Self-hosted-Plans-and-Pricing), or your self-hosted admin panel, or our official website ([link](https://appflowy.com/pricing)).

Open source vs Open core:

As AppFlwy Cloud is adopting an open-core model, AppFlowy Web and AppFlowy Flutter will remain open source.
As part of the transition, we are consolidating and reorganizing our codebases in private repositories.
Recently, we merged all AppFlowy Web–related development from our private repository back into the public one. You can check the [commit](https://github.com/AppFlowy-IO/AppFlowy-Web/commits/main/) for reference. We will also merge Flutter code back into [this](https://github.com/AppFlowy-IO/AppFlowy) public repository at a later stage.

AppFlowy Cloud will continue to follow the open-core model for business reasons.
You're free to use https://github.com/AppFlowy-IO/AppFlowy-Cloud governed by its license.

We also have a few other open source repos, such as

- [AppFlowy Website](https://github.com/AppFlowy-IO/AppFlowy-Website) for developers who want to build their own website following our navigation structure
- [appflowy-editor](https://github.com/AppFlowy-IO/appflowy-editor) for Flutter developers to build their own apps
- [appflowy-board](https://github.com/AppFlowy-IO/appflowy-board) for Flutter developers to build their own apps



## Table of Contents

- [🚀 Deployment](#deployment)


## 🚀Deployment

- See [deployment guide](https://appflowy.com/docs/Step-by-step-Self-Hosting-Guide---From-Zero-to-Production)

