---
category: concepts
slug: concepts/performances
title: Performances
description: How we track and optimize the performance of Yarn.
sidebar:
  order: 6
---

Performances are an important aspect of Yarn's design and development, especially as of Yarn 6.

To track our progress and make sure we don't accidentally regress, we have our CI run automated performance tests multiple times a day every day, across all major package managers.

What follows is only a small subset of the various scenarios we benchmark. To see all variants of these tests, consult our dedicated [Datadog dashboard](https://yarnpkg.com/benchmarks).

<iframe
  src="https://app.datadoghq.eu/graph/embed?token=417dae5b1bbcc11cddd9b4a86420ee808dd3e91f0c1d0f2ae67754c54ef3871c&height=300&width=600&legend=true"
  style="width: 600px; height: 300px;"
  frameborder="0"
></iframe>

<iframe
  src="https://app.datadoghq.eu/graph/embed?token=0668b77c5fd00c76a59378edea1c4a93e22cd2442c5854e3b0032aeede4037cc&height=300&width=600&legend=true"
  style="width: 600px; height: 300px;"
  frameborder="0"
></iframe>
