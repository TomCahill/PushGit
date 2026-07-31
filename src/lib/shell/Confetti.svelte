<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // Renders whatever `confetti.svelte.ts`'s `confettiState.bursts` currently holds, onto one
  // shared full-viewport canvas — mounted once at the shell root like `Toast`/`ConfirmDialog`/
  // `ContextMenu`. Reduce-motion is handled entirely by `burstConfetti()` refusing to queue a
  // burst in the first place (`.private/feature/push-confetti/PLAN.md`), so this component never
  // has to check it itself.
  import { confettiState, endBurst, type ConfettiBurst } from "./confetti.svelte";

  // A dedicated vivid palette rather than the app's `--accent`/`--success`/`--danger`/`--warning`
  // tokens — those are muted, theme-matched UI colors, four hues total, and read as flat/dull for
  // a celebratory burst. Confetti wants many saturated colors regardless of light/dark theme.
  const PALETTE = [
    "#f94144",
    "#f3722c",
    "#f8961e",
    "#f9c74f",
    "#90be6d",
    "#43aa8b",
    "#4d908e",
    "#577590",
    "#277da1",
    "#e63b8f",
  ];
  const PARTICLES_PER_BURST = 90;
  const GRAVITY_PX_PER_S2 = 1400;
  const DRAG_PER_FRAME = 0.98;
  const MAX_LIFETIME_MS = 1400;

  type Particle = {
    burstId: number;
    x: number;
    y: number;
    vx: number;
    vy: number;
    rotation: number;
    rotationSpeed: number;
    size: number;
    color: string;
    bornAt: number;
  };

  let canvasEl = $state<HTMLCanvasElement | null>(null);
  let particles: Particle[] = [];
  // Bursts this component has already spawned particles for — separate from
  // `confettiState.bursts` itself, which this component also removes from (via `endBurst`) once
  // a burst's particles have all died out.
  const seenBurstIds = new Set<number>();
  let rafHandle: number | null = null;
  let lastFrameTime: number | null = null;

  function spawnParticles(burst: ConfettiBurst): void {
    const bornAt = performance.now();
    for (let i = 0; i < PARTICLES_PER_BURST; i++) {
      // Upward-biased cone (mostly straight up, +/- ~100deg) rather than a full circle, so it
      // reads as bursting "out of" the button rather than raining from a point.
      const angle = -Math.PI / 2 + (Math.random() - 0.5) * (Math.PI * 1.1);
      const speed = 280 + Math.random() * 420;
      particles.push({
        burstId: burst.id,
        x: burst.x,
        y: burst.y,
        vx: Math.cos(angle) * speed,
        vy: Math.sin(angle) * speed,
        rotation: Math.random() * Math.PI * 2,
        rotationSpeed: (Math.random() - 0.5) * 14,
        size: 4 + Math.random() * 5,
        color: PALETTE[Math.floor(Math.random() * PALETTE.length)],
        bornAt,
      });
    }
    ensureLoopRunning();
  }

  function resizeCanvas(): void {
    if (!canvasEl) return;
    canvasEl.width = window.innerWidth;
    canvasEl.height = window.innerHeight;
  }

  function ensureLoopRunning(): void {
    if (rafHandle !== null) return;
    lastFrameTime = null;
    rafHandle = requestAnimationFrame(tick);
  }

  function tick(now: number): void {
    const dt = lastFrameTime === null ? 0 : (now - lastFrameTime) / 1000;
    lastFrameTime = now;

    // jsdom (unit tests) has no canvas 2D context support — the particle simulation and burst
    // lifecycle below still run without it, only the drawing itself is skipped.
    const ctx = canvasEl?.getContext("2d") ?? null;
    if (ctx && canvasEl) ctx.clearRect(0, 0, canvasEl.width, canvasEl.height);

    particles = particles.filter((particle) => {
      const age = now - particle.bornAt;
      if (age > MAX_LIFETIME_MS) return false;
      if (canvasEl && particle.y - particle.size > canvasEl.height) return false;

      particle.vy += GRAVITY_PX_PER_S2 * dt;
      particle.vx *= DRAG_PER_FRAME;
      particle.vy *= DRAG_PER_FRAME;
      particle.x += particle.vx * dt;
      particle.y += particle.vy * dt;
      particle.rotation += particle.rotationSpeed * dt;

      if (ctx) {
        ctx.save();
        ctx.translate(particle.x, particle.y);
        ctx.rotate(particle.rotation);
        ctx.globalAlpha = Math.max(0, 1 - age / MAX_LIFETIME_MS);
        ctx.fillStyle = particle.color;
        ctx.fillRect(-particle.size / 2, -particle.size / 2, particle.size, particle.size);
        ctx.restore();
      }

      return true;
    });

    for (const burstId of seenBurstIds) {
      if (!particles.some((particle) => particle.burstId === burstId)) {
        seenBurstIds.delete(burstId);
        endBurst(burstId);
      }
    }

    if (particles.length > 0) {
      rafHandle = requestAnimationFrame(tick);
    } else {
      rafHandle = null;
      lastFrameTime = null;
    }
  }

  $effect(() => {
    for (const burst of confettiState.bursts) {
      if (!seenBurstIds.has(burst.id)) {
        seenBurstIds.add(burst.id);
        spawnParticles(burst);
      }
    }
  });

  $effect(() => {
    resizeCanvas();
    window.addEventListener("resize", resizeCanvas);
    return () => {
      window.removeEventListener("resize", resizeCanvas);
      if (rafHandle !== null) cancelAnimationFrame(rafHandle);
    };
  });
</script>

<canvas bind:this={canvasEl} class="confetti-canvas" aria-hidden="true"></canvas>

<style>
  .confetti-canvas {
    position: fixed;
    inset: 0;
    z-index: 300;
    pointer-events: none;
  }
</style>
