-- DRAFT for the planned cloud sync. Not applied to any project yet.
--
-- Cardboard Capital's online storage, planned on Supabase (Postgres), starting on the free tier.
-- Plain relational tables, so the data moves to a paid plan, or to Neon/RDS, with pg_dump and
-- restore. Only `auth.users` / `auth.uid()` are Supabase-specific (its Auth service).
--
-- Who is a developer is decided here (profiles.role, set by the owner in the dashboard), never by
-- the app's local developer.key. During the preview only developers can read or write data.
--
-- Apply later with the Supabase CLI: `supabase link --project-ref <ref>`, then `supabase db push`.

-- One row per signed-up user.
create table public.profiles (
  id uuid primary key references auth.users (id) on delete cascade,
  role text not null default 'user' check (role in ('user', 'developer')),
  created_at timestamptz not null default now()
);

-- Every new user gets a profile (role 'user').
create function public.handle_new_user()
returns trigger
language plpgsql
security definer set search_path = ''
as $$
begin
  insert into public.profiles (id) values (new.id);
  return new;
end;
$$;

create trigger on_auth_user_created
  after insert on auth.users
  for each row execute function public.handle_new_user();

-- Whether the signed-in user is a developer. Security definer, so policies can check it without
-- letting anyone read other people's profiles.
create function public.is_developer()
returns boolean
language sql
stable
security definer set search_path = ''
as $$
  select exists (
    select 1 from public.profiles
    where id = (select auth.uid()) and role = 'developer'
  );
$$;

create function public.touch_updated_at()
returns trigger
language plpgsql
set search_path = ''
as $$
begin
  new.updated_at = now();
  return new;
end;
$$;

-- Settings collections depend on: manual prices are stored in this currency.
create table public.user_settings (
  user_id uuid primary key references auth.users (id) on delete cascade,
  currency text not null default 'SGD' check (char_length(currency) = 3),
  updated_at timestamptz not null default now()
);

-- One row per owned stack: one printing in one finish and condition (the app's add_or_merge
-- rule). Card details (name, set, images, market price) aren't stored; they're re-fetched from the
-- game's card source (Scryfall for MTG), which keeps rows small. The unique constraint's index
-- also serves "this user's cards for this game".
create table public.collection_entries (
  id uuid primary key default gen_random_uuid(),
  user_id uuid not null references auth.users (id) on delete cascade,
  game text not null check (game in ('mtg')),
  card_id text not null,                                   -- Scryfall id for MTG
  finish text not null,                                    -- NonFoil | Foil | Etched
  condition text not null,                                 -- NM | LP | MP | HP | DMG
  qty integer not null check (qty > 0),
  manual_price numeric(12, 2) check (manual_price >= 0),   -- in user_settings.currency
  treatment_override text not null default '',
  notes text not null default '',
  checked boolean not null default false,
  added_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  unique (user_id, game, card_id, finish, condition)
);

create trigger user_settings_touch
  before update on public.user_settings
  for each row execute function public.touch_updated_at();

create trigger collection_entries_touch
  before update on public.collection_entries
  for each row execute function public.touch_updated_at();

alter table public.profiles enable row level security;
alter table public.user_settings enable row level security;
alter table public.collection_entries enable row level security;

-- Everyone can see their own profile. Roles are only changed from the dashboard.
create policy "Users read their own profile"
  on public.profiles for select to authenticated
  using (id = (select auth.uid()));

-- Preview: only developers, and only their own rows. To open up to everyone, replace these in a
-- new migration without the is_developer() check.
create policy "Developers manage their own settings"
  on public.user_settings for all to authenticated
  using (user_id = (select auth.uid()) and (select public.is_developer()))
  with check (user_id = (select auth.uid()) and (select public.is_developer()));

create policy "Developers manage their own collection"
  on public.collection_entries for all to authenticated
  using (user_id = (select auth.uid()) and (select public.is_developer()))
  with check (user_id = (select auth.uid()) and (select public.is_developer()));
