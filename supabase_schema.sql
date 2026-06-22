-- h00d.fun — Supabase schema + Row-Level Security
-- =============================================================================
-- Run this ONCE in the Supabase SQL editor before going live. The site ships a
-- PUBLIC anon key (in index.html / discover.html / launch.html) — that is normal
-- for Supabase, but it is ONLY safe if the RLS policies below are in place. Without
-- them, anyone holding the (public) anon key could read every private DM and
-- insert/modify/delete other users' rows.
--
-- Identity model: auth is keyed on auth.uid() (a real Supabase user), but the app
-- stores a human `username` on posts/messages. The policies below BIND that
-- username to the authenticated user via current_username(), so a client cannot
-- post or DM as someone else.
--
-- IMPORTANT: the app signs users up with a synthetic email (username@h00d.fun) and
-- inserts their profile row immediately after signUp(). For that insert to satisfy
-- RLS, the signUp must return an active session — so disable "Confirm email" in
-- Supabase → Authentication → Providers → Email.
-- =============================================================================

create extension if not exists pgcrypto;   -- gen_random_uuid()

-- ── tables ───────────────────────────────────────────────────────────────────

create table if not exists public.profiles (
  id         uuid primary key references auth.users(id) on delete cascade,
  username   text unique not null
             check (username = lower(username) and char_length(username) between 1 and 24),
  wallet     text
             check (wallet is null or wallet ~ '^[1-9A-HJ-NP-Za-km-z]{32,44}$'),
  created_at timestamptz not null default now()
);

create table if not exists public.posts (
  id         uuid primary key default gen_random_uuid(),
  user_id    uuid not null references auth.users(id) on delete cascade,
  username   text not null,
  body       text not null check (char_length(body) between 1 and 5000),
  parent_id  uuid references public.posts(id) on delete cascade,
  created_at timestamptz not null default now()
);

create table if not exists public.messages (
  id         uuid primary key default gen_random_uuid(),
  sender_id  uuid not null references auth.users(id) on delete cascade,
  sender     text not null,
  recipient  text not null,
  body       text not null check (char_length(body) between 1 and 2000),
  created_at timestamptz not null default now()
);

create table if not exists public.launches (
  id         uuid primary key default gen_random_uuid(),
  created_by uuid default auth.uid() references auth.users(id) on delete set null,
  mint       text not null,
  name       text,
  symbol     text,
  creator    text,
  decimals   int,
  supply     text,
  uri        text,
  renounced  boolean default false,
  sig        text,
  created_at timestamptz not null default now()
);

-- ── indexes ──────────────────────────────────────────────────────────────────

create index if not exists posts_parent_idx   on public.posts(parent_id);
create index if not exists posts_created_idx   on public.posts(created_at desc);
create index if not exists posts_username_idx  on public.posts(username);
create index if not exists messages_sender_idx    on public.messages(sender);
create index if not exists messages_recipient_idx on public.messages(recipient);
create index if not exists launches_created_idx on public.launches(created_at desc);

-- ── helper: the username of the current authenticated user ───────────────────
-- security definer so the policy can read profiles regardless of the caller's RLS.

create or replace function public.current_username()
returns text
language sql
stable
security definer
set search_path = public
as $$
  select username from public.profiles where id = auth.uid();
$$;

-- ── enable RLS ───────────────────────────────────────────────────────────────

alter table public.profiles enable row level security;
alter table public.posts    enable row level security;
alter table public.messages enable row level security;
alter table public.launches enable row level security;

-- ── policies: profiles (public read; you may only write your own) ────────────

drop policy if exists profiles_read   on public.profiles;
drop policy if exists profiles_insert on public.profiles;
drop policy if exists profiles_update on public.profiles;

create policy profiles_read   on public.profiles for select to anon, authenticated using (true);
create policy profiles_insert on public.profiles for insert to authenticated with check (id = auth.uid());
create policy profiles_update on public.profiles for update to authenticated using (id = auth.uid()) with check (id = auth.uid());

-- ── policies: posts (public read; insert/delete only your own, no spoofing) ──

drop policy if exists posts_read   on public.posts;
drop policy if exists posts_insert on public.posts;
drop policy if exists posts_delete on public.posts;

create policy posts_read   on public.posts for select to anon, authenticated using (true);
create policy posts_insert on public.posts for insert to authenticated
  with check (user_id = auth.uid() and username = public.current_username());
create policy posts_delete on public.posts for delete to authenticated
  using (user_id = auth.uid());

-- ── policies: messages (PRIVATE — only the two participants, never anon) ─────

drop policy if exists messages_read   on public.messages;
drop policy if exists messages_insert on public.messages;

create policy messages_read on public.messages for select to authenticated
  using (sender = public.current_username() or recipient = public.current_username());
create policy messages_insert on public.messages for insert to authenticated
  with check (sender_id = auth.uid()
              and sender = public.current_username()
              and recipient <> sender);

-- ── policies: launches (public directory; insert only as yourself) ───────────

drop policy if exists launches_read   on public.launches;
drop policy if exists launches_insert on public.launches;

create policy launches_read   on public.launches for select to anon, authenticated using (true);
create policy launches_insert on public.launches for insert to authenticated
  with check (created_by = auth.uid());

-- ── role grants (RLS still applies on top of these) ──────────────────────────

grant usage on schema public to anon, authenticated;
grant select on public.profiles, public.posts, public.launches to anon, authenticated;
grant insert, update on public.profiles to authenticated;
grant insert, delete on public.posts     to authenticated;
grant insert         on public.launches  to authenticated;
-- messages: authenticated only, never anon
grant select, insert on public.messages  to authenticated;
grant execute on function public.current_username() to authenticated;
