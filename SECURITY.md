# Security Policy

## Supported versions

molde is pre-1.0 (`0.0.x`). Security fixes are applied to the latest released
version on the `main` branch. Until 1.0, older releases are not patched
separately — please upgrade to the latest version.

## Reporting a vulnerability

**Please do not report security vulnerabilities through public GitHub issues.**

Instead, use one of:

- GitHub's [private vulnerability reporting](https://github.com/mawesi/molde/security/advisories/new)
  ("Report a vulnerability" under the *Security* tab), or
- email **mauricio@mawesi.net** with the details.

Please include:

- a description of the issue and its impact,
- steps to reproduce (a minimal `.model` file or command is ideal),
- affected version / commit, and
- any suggested remediation if you have one.

You can expect an acknowledgement within a few business days. We will work with
you to understand and validate the report, prepare a fix, and coordinate
disclosure. Please give us a reasonable window to release a fix before any public
disclosure.

## Scope

molde connects to databases and runs DDL it generates from your models and
migrations. When reporting, areas of particular interest include: SQL generation
that could be coerced into unintended statements, handling of connection strings
and credentials, and parsing of untrusted `.model` / migration files.
