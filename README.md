# Scyros

[![Actions status](https://github.com/fxpl/scyros/actions/workflows/ci.yml/badge.svg)](https://github.com/fxpl/scyros/actions)
[![Rust](https://img.shields.io/badge/rust-1.85-blue)](
https://releases.rs/docs/1.85.0/
)

A framework to design sound, reproducible and scalable mining repositories studies on GitHub.

### Scyros is...

- 🧪 **Reproducibility-first**: declarative configuration and deterministic execution to enable repeatable experiments.
- 📈 **Scalable**: designed for large-scale repository mining studies on GitHub.
- 🧱 **Soundness-focused**: encourages transparent, bias-aware, and methodologically explicit study design.
- ⚙️ **Modular**: independent, reusable modules that can be composed into custom data-processing pipelines.

## Table of Contents

- [Installation](#installation)
- [Usage](#usage)
- [Authentication and Rate Limits](#authentication-and-rate-limits)
- [Citing Scyros](#citing-scyros)
- [License](#license)


## Installation

This project is written is Rust and requires Rust version 1.85. Install Rust by following the instructions on the [official website](https://rust-lang.org/tools/install/).

Build Scyros from source:
```bash
git clone git@github.com:fxpl/scyros.git
cd scyros
cargo build --release
```

The binary is produced at `target/release/scyros`. You can optionally move it to a directory in your PATH for easier access.

## Usage

To discover available commands and modules:

```bash
scyros --help
```

Each module provides its own usage documentation. For example, to inspect the module used to sample random repositories from GitHub:

```bash
scyros ids --help
```

## 

Some modules interact with the GitHub API and require personal access tokens (PATs). Tokens can be created by following GitHub’s documentation: [https://docs.github.com/en/github/authenticating-to-github/creating-a-personal-access-token](https://docs.github.com/en/github/authenticating-to-github/creating-a-personal-access-token).

⚠️ Never commit or share your tokens publicly.

Tokens must be provided as a CSV file passed via a command-line argument. The file must contain a single column named token, with one token per line:
```csv
    token
    fa56454....
    hj73647.... 
```

GitHub enforces API rate limits. Using multiple tokens from the same account does not increase these limits. Users are expected to comply with GitHub’s API terms and rate-limit policies:
- [Rate limits for the REST API](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api?apiVersion=2022-11-28)
- [Terms of Service](https://docs.github.com/en/site-policy/github-terms/github-terms-of-service)

## Citing Scyros

Scyros is introduced and described in the following large-scale empirical study. If you use Scyros in academic work, please cite:.

```bibtex
@misc{gilot2025largescalestudyfloatingpointusage,
    title={A Large-Scale Study of Floating-Point Usage in Statically Typed Languages}, 
    author={Andrea Gilot and Tobias Wrigstad and Eva Darulova},
    year={2025},
    eprint={2509.04936},
    archivePrefix={arXiv},
    primaryClass={cs.PL},
    url={https://arxiv.org/abs/2509.04936}, 
}   
```
> Gilot, A., Wrigstad, T., & Darulova, E. (2025). A Large-Scale Study of Floating-Point Usage in Statically Typed Languages. arXiv. https://arxiv.org/abs/2509.04936

## License
This project is licensed under the Apache License 2.0. See [LICENSE](LICENSE) for details.