--
-- PostgreSQL database dump
--

\restrict smLsFw4SxJ9gqvYN4OPg3fkdfmBBmf9Wbjz9ddz1RSF1mvDrMcQ5xhqMvizhMiG

-- Dumped from database version 17.6
-- Dumped by pg_dump version 17.6

SET statement_timeout = 0;
SET lock_timeout = 0;
SET idle_in_transaction_session_timeout = 0;
SET transaction_timeout = 0;
SET client_encoding = 'UTF8';
SET standard_conforming_strings = on;
SELECT pg_catalog.set_config('search_path', '', false);
SET check_function_bodies = false;
SET xmloption = content;
SET client_min_messages = warning;
SET row_security = off;

--
-- Name: citext; Type: EXTENSION; Schema: -; Owner: -
--

CREATE EXTENSION IF NOT EXISTS citext WITH SCHEMA public;


--
-- Name: EXTENSION citext; Type: COMMENT; Schema: -; Owner: 
--

COMMENT ON EXTENSION citext IS 'data type for case-insensitive character strings';


--
-- Name: pgcrypto; Type: EXTENSION; Schema: -; Owner: -
--

CREATE EXTENSION IF NOT EXISTS pgcrypto WITH SCHEMA public;


--
-- Name: EXTENSION pgcrypto; Type: COMMENT; Schema: -; Owner: 
--

COMMENT ON EXTENSION pgcrypto IS 'cryptographic functions';


--
-- Name: lab_fun_audit(uuid, text, text, jsonb, text, text); Type: FUNCTION; Schema: public; Owner: postgres
--

CREATE FUNCTION public.lab_fun_audit(p_user_id uuid, p_action text, p_target text, p_meta jsonb, p_ip text, p_user_agent text) RETURNS void
    LANGUAGE plpgsql
    AS $$
BEGIN
  INSERT INTO lab_audit_logs(user_id, action, target, meta, ip_addr, user_agent)
  VALUES (p_user_id, p_action, NULLIF(p_target,''), p_meta, NULLIF(p_ip,''), NULLIF(p_user_agent,''));
END;
$$;


ALTER FUNCTION public.lab_fun_audit(p_user_id uuid, p_action text, p_target text, p_meta jsonb, p_ip text, p_user_agent text) OWNER TO postgres;

--
-- Name: lab_fun_consume_refresh_token(uuid, bytea, bytea, timestamp with time zone, text, text); Type: FUNCTION; Schema: public; Owner: postgres
--

CREATE FUNCTION public.lab_fun_consume_refresh_token(p_user_id uuid, p_current_sha256 bytea, p_new_sha256 bytea, p_new_expires_at timestamp with time zone, p_user_agent text, p_ip_addr text) RETURNS uuid
    LANGUAGE plpgsql
    AS $$
DECLARE
    v_old_token_id uuid;
    v_new_token_id uuid := gen_random_uuid();
BEGIN
    -- lock row
    SELECT token_id INTO v_old_token_id
    FROM lab_refresh_tokens
    WHERE user_id = p_user_id
      AND token_sha256 = p_current_sha256
      AND revoked = false
      AND now() < expires_at
    FOR UPDATE;

    IF v_old_token_id IS NULL THEN
        RAISE EXCEPTION 'REFRESH_INVALID_OR_EXPIRED';
    END IF;

    UPDATE lab_refresh_tokens
       SET revoked = true
     WHERE token_id = v_old_token_id;

    INSERT INTO lab_refresh_tokens(token_id, user_id, token_sha256, user_agent, ip_addr, expires_at, revoked, rotated_from)
    VALUES (v_new_token_id, p_user_id, p_new_sha256, p_user_agent, p_ip_addr, p_new_expires_at, false, v_old_token_id);

    RETURN v_new_token_id;
END;
$$;


ALTER FUNCTION public.lab_fun_consume_refresh_token(p_user_id uuid, p_current_sha256 bytea, p_new_sha256 bytea, p_new_expires_at timestamp with time zone, p_user_agent text, p_ip_addr text) OWNER TO postgres;

--
-- Name: lab_fun_create_refresh_token(uuid, bytea, text, text, timestamp with time zone); Type: FUNCTION; Schema: public; Owner: postgres
--

CREATE FUNCTION public.lab_fun_create_refresh_token(p_user_id uuid, p_token_sha256 bytea, p_user_agent text, p_ip_addr text, p_expires_at timestamp with time zone) RETURNS uuid
    LANGUAGE plpgsql
    AS $$
DECLARE
    v_token_id uuid := gen_random_uuid();
BEGIN
    INSERT INTO lab_refresh_tokens(token_id, user_id, token_sha256, user_agent, ip_addr, expires_at, revoked)
    VALUES (v_token_id, p_user_id, p_token_sha256, p_user_agent, p_ip_addr, p_expires_at, false);
    RETURN v_token_id;
END;
$$;


ALTER FUNCTION public.lab_fun_create_refresh_token(p_user_id uuid, p_token_sha256 bytea, p_user_agent text, p_ip_addr text, p_expires_at timestamp with time zone) OWNER TO postgres;

--
-- Name: lab_fun_deposit(uuid, uuid, double precision, text); Type: FUNCTION; Schema: public; Owner: postgres
--

CREATE FUNCTION public.lab_fun_deposit(p_user_id uuid, p_account_id uuid, p_amount double precision, p_description text) RETURNS TABLE(journal_id uuid, account_id uuid, balance_after double precision, trx_time timestamp with time zone, description text)
    LANGUAGE plpgsql
    AS $$
DECLARE
  v_owner uuid;
  v_balance double precision;
  v_journal uuid := gen_random_uuid();
  v_desc text := COALESCE(p_description, 'Setor tunai');
BEGIN
  IF p_amount IS NULL OR p_amount <= 0 THEN
    RAISE EXCEPTION 'AMOUNT_INVALID';
  END IF;

  SELECT user_id INTO v_owner FROM lab_accounts WHERE id = p_account_id;
  IF v_owner IS NULL OR v_owner <> p_user_id THEN
    RAISE EXCEPTION 'ACCOUNT_NOT_OWNED';
  END IF;

  -- tambah saldo
  UPDATE lab_accounts
     SET saldo = saldo + p_amount
   WHERE id = p_account_id
   RETURNING saldo INTO v_balance;

  -- jurnal debit
  INSERT INTO lab_journals(id, user_id, account_id, debit, credit, description, balance_after, trx_time)
  VALUES (v_journal, p_user_id, p_account_id, p_amount, 0, v_desc, v_balance, now());

  INSERT INTO lab_audit_logs(id, user_id, action, ip_addr, user_agent, created_at)
  VALUES (gen_random_uuid(), p_user_id, 'DEPOSIT', NULL, NULL, now());

  RETURN QUERY
  SELECT v_journal, p_account_id, v_balance, now(), v_desc;
END;
$$;


ALTER FUNCTION public.lab_fun_deposit(p_user_id uuid, p_account_id uuid, p_amount double precision, p_description text) OWNER TO postgres;

--
-- Name: lab_fun_find_refresh_token(uuid, text); Type: FUNCTION; Schema: public; Owner: postgres
--

CREATE FUNCTION public.lab_fun_find_refresh_token(p_user_id uuid, p_token_hash text) RETURNS TABLE(token_id uuid, expires_at timestamp with time zone, revoked_at timestamp with time zone)
    LANGUAGE sql
    AS $$
  SELECT id, expires_at, revoked_at
  FROM lab_refresh_tokens
  WHERE user_id = p_user_id AND token_hash = p_token_hash
  ORDER BY expires_at DESC
  LIMIT 1;
$$;


ALTER FUNCTION public.lab_fun_find_refresh_token(p_user_id uuid, p_token_hash text) OWNER TO postgres;

--
-- Name: lab_fun_generate_account_no(); Type: FUNCTION; Schema: public; Owner: postgres
--

CREATE FUNCTION public.lab_fun_generate_account_no() RETURNS text
    LANGUAGE plpgsql
    AS $$
DECLARE
  v_no text;
BEGIN
  v_no := to_char(now(), 'YYYYMMDD') || lpad(((random()*1000000)::int)::text, 6, '0');
  RETURN v_no;
END; $$;


ALTER FUNCTION public.lab_fun_generate_account_no() OWNER TO postgres;

--
-- Name: lab_fun_get_profile(uuid); Type: FUNCTION; Schema: public; Owner: postgres
--

CREATE FUNCTION public.lab_fun_get_profile(p_user_id uuid) RETURNS TABLE(user_id uuid, ktp_nik text, nama_lengkap text, tempat_lahir text, tanggal_lahir date, jenis_kelamin text, no_telepon text, alamat text, ibu_kandung text, updated_at timestamp with time zone)
    LANGUAGE sql
    AS $$
  SELECT user_id, ktp_nik, nama_lengkap, tempat_lahir, tanggal_lahir,
         jenis_kelamin, no_telepon, alamat, ibu_kandung, updated_at
  FROM lab_profiles
  WHERE user_id = p_user_id;
$$;


ALTER FUNCTION public.lab_fun_get_profile(p_user_id uuid) OWNER TO postgres;

--
-- Name: lab_fun_get_user_auth(text); Type: FUNCTION; Schema: public; Owner: postgres
--

CREATE FUNCTION public.lab_fun_get_user_auth(p_email text) RETURNS TABLE(user_id uuid, password_hash text, role text, is_active boolean)
    LANGUAGE sql
    AS $$
  SELECT id, password_hash, role, is_active
  FROM lab_users
  WHERE email = p_email;
$$;


ALTER FUNCTION public.lab_fun_get_user_auth(p_email text) OWNER TO postgres;

--
-- Name: lab_fun_journal_guard(); Type: FUNCTION; Schema: public; Owner: postgres
--

CREATE FUNCTION public.lab_fun_journal_guard() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
  IF COALESCE(NEW.debit,0) < 0 OR COALESCE(NEW.credit,0) < 0 THEN
    RAISE EXCEPTION 'NEGATIVE_AMOUNT';
  END IF;
  IF COALESCE(NEW.debit,0) = 0 AND COALESCE(NEW.credit,0) = 0 THEN
    RAISE EXCEPTION 'ZERO_BOTH';
  END IF;
  RETURN NEW;
END;
$$;


ALTER FUNCTION public.lab_fun_journal_guard() OWNER TO postgres;

--
-- Name: lab_fun_list_accounts_by_user(uuid); Type: FUNCTION; Schema: public; Owner: postgres
--

CREATE FUNCTION public.lab_fun_list_accounts_by_user(p_user_id uuid) RETURNS TABLE(id uuid, account_no text, saldo numeric, created_at timestamp with time zone, updated_at timestamp with time zone)
    LANGUAGE sql STABLE
    AS $$
  SELECT a.id, a.account_no, a.saldo, a.created_at, a.updated_at
  FROM lab_accounts a
  WHERE a.user_id = p_user_id
  ORDER BY a.created_at ASC;
$$;


ALTER FUNCTION public.lab_fun_list_accounts_by_user(p_user_id uuid) OWNER TO postgres;

--
-- Name: lab_fun_list_journal(uuid, uuid, integer, integer); Type: FUNCTION; Schema: public; Owner: postgres
--

CREATE FUNCTION public.lab_fun_list_journal(p_user_id uuid, p_account_id uuid, p_limit integer, p_offset integer) RETURNS TABLE(id uuid, trx_time timestamp with time zone, debit numeric, credit numeric, description text, balance_after numeric)
    LANGUAGE sql STABLE
    AS $$
  SELECT j.id, j.trx_time, j.debit, j.credit, j.description, j.balance_after
  FROM lab_journals j
  WHERE j.user_id = p_user_id
    AND j.account_id = p_account_id
  ORDER BY j.trx_time DESC
  LIMIT GREATEST(p_limit,1)
  OFFSET GREATEST(p_offset,0);
$$;


ALTER FUNCTION public.lab_fun_list_journal(p_user_id uuid, p_account_id uuid, p_limit integer, p_offset integer) OWNER TO postgres;

--
-- Name: lab_fun_open_account(uuid, text, double precision); Type: FUNCTION; Schema: public; Owner: postgres
--

CREATE FUNCTION public.lab_fun_open_account(p_user_id uuid, p_pin text, p_initial double precision) RETURNS TABLE(account_id uuid, account_no text)
    LANGUAGE plpgsql
    AS $_$
DECLARE
  v_id uuid;
  v_no text;
  v_retry int := 0;
BEGIN
  IF p_pin IS NULL OR length(p_pin) <> 6 OR p_pin !~ '^[0-9]{6}$' THEN
    RAISE EXCEPTION 'PIN_INVALID';
  END IF;

  IF p_initial IS NOT NULL AND p_initial < 0 THEN
    RAISE EXCEPTION 'INITIAL_NEGATIVE';
  END IF;

  -- generate nomor unik & insert akun
  LOOP
    v_no := lab_fun_generate_account_no();
    BEGIN
      INSERT INTO lab_accounts(user_id, account_no, pin, saldo)
      VALUES (
        p_user_id,
        v_no,
        crypt(p_pin, gen_salt('bf')),  -- <<< HASH PIN DI SINI
        0
      )
      RETURNING id INTO v_id;

      EXIT; -- sukses insert
    EXCEPTION WHEN unique_violation THEN
      v_retry := v_retry + 1;
      IF v_retry > 5 THEN
        RAISE EXCEPTION 'GEN_ACCOUNT_NO_FAILED';
      END IF;
    END;
  END LOOP;

  -- setoran awal (opsional)
  IF p_initial IS NOT NULL AND p_initial > 0 THEN
    UPDATE lab_accounts
       SET saldo = saldo + p_initial
     WHERE id = v_id;

    INSERT INTO lab_journals(user_id, account_id, debit, credit, description, balance_after)
    VALUES (
      p_user_id,
      v_id,
      p_initial,
      0,
      'Setoran awal',
      (SELECT saldo FROM lab_accounts WHERE id = v_id)
    );
  END IF;

  RETURN QUERY SELECT v_id, v_no;
END;
$_$;


ALTER FUNCTION public.lab_fun_open_account(p_user_id uuid, p_pin text, p_initial double precision) OWNER TO postgres;

--
-- Name: lab_fun_post_journal(uuid, uuid, double precision, double precision, text); Type: FUNCTION; Schema: public; Owner: postgres
--

CREATE FUNCTION public.lab_fun_post_journal(p_user_id uuid, p_account_id uuid, p_debit double precision, p_credit double precision, p_description text) RETURNS uuid
    LANGUAGE plpgsql
    AS $$
DECLARE
  v_owner   uuid;
  v_balance numeric;
  v_newbal  numeric;
  v_id      uuid := gen_random_uuid();
BEGIN
  -- Normalisasi nilai NULL → 0
  p_debit  := COALESCE(p_debit,  0);
  p_credit := COALESCE(p_credit, 0);

  -- Validasi dasar
  IF p_debit < 0 OR p_credit < 0 THEN
    RAISE EXCEPTION 'AMOUNT_NEGATIVE';
  END IF;
  IF (p_debit = 0 AND p_credit = 0) OR (p_debit > 0 AND p_credit > 0) THEN
    RAISE EXCEPTION 'AMOUNT_INVALID';
  END IF;

  -- Pastikan akun milik user & lock baris untuk update saldo
  SELECT user_id, saldo
    INTO v_owner, v_balance
  FROM lab_accounts
  WHERE id = p_account_id
  FOR UPDATE;

  IF v_owner IS NULL THEN
    RAISE EXCEPTION 'ACCOUNT_NOT_FOUND';
  END IF;
  IF v_owner <> p_user_id THEN
    RAISE EXCEPTION 'ACCOUNT_NOT_OWNED';
  END IF;

  -- Jika penarikan (credit), cek saldo cukup
  IF p_credit > 0 AND v_balance < p_credit THEN
    RAISE EXCEPTION 'INSUFFICIENT_FUNDS';
  END IF;

  -- Hitung saldo baru (saldo numeric, argumen double → cast)
  v_newbal := v_balance + (p_debit::numeric) - (p_credit::numeric);

  -- Update saldo akun
  UPDATE lab_accounts
     SET saldo = v_newbal
   WHERE id = p_account_id;

  -- Catat jurnal
  INSERT INTO lab_journals(
    id, user_id, account_id, debit, credit, description, balance_after, trx_time
  )
  VALUES (
    v_id, p_user_id, p_account_id,
    p_debit::numeric, p_credit::numeric,
    COALESCE(p_description, ''),
    v_newbal,
    now()
  );

  RETURN v_id;
END;
$$;


ALTER FUNCTION public.lab_fun_post_journal(p_user_id uuid, p_account_id uuid, p_debit double precision, p_credit double precision, p_description text) OWNER TO postgres;

--
-- Name: lab_fun_register_user(text, text, text, text, text); Type: FUNCTION; Schema: public; Owner: postgres
--

CREATE FUNCTION public.lab_fun_register_user(p_email text, p_password_hash text, p_role text, p_user_agent text, p_ip text) RETURNS uuid
    LANGUAGE plpgsql
    AS $$
DECLARE v_id uuid;
BEGIN
  IF EXISTS (SELECT 1 FROM lab_users WHERE email = p_email) THEN
    RAISE EXCEPTION 'EMAIL_EXISTS';
  END IF;

  INSERT INTO lab_users(email, password_hash, role)
  VALUES (p_email, p_password_hash, COALESCE(p_role,'user'))
  RETURNING id INTO v_id;

  RETURN v_id;
END; $$;


ALTER FUNCTION public.lab_fun_register_user(p_email text, p_password_hash text, p_role text, p_user_agent text, p_ip text) OWNER TO postgres;

--
-- Name: lab_fun_revoke_access_token(uuid, timestamp with time zone); Type: FUNCTION; Schema: public; Owner: postgres
--

CREATE FUNCTION public.lab_fun_revoke_access_token(p_jti uuid, p_expires_at timestamp with time zone) RETURNS boolean
    LANGUAGE plpgsql
    AS $$
BEGIN
  INSERT INTO lab_revoked_access_tokens(jti, expires_at)
  VALUES (p_jti, p_expires_at)
  ON CONFLICT (jti) DO NOTHING;

  RETURN true;
END;
$$;


ALTER FUNCTION public.lab_fun_revoke_access_token(p_jti uuid, p_expires_at timestamp with time zone) OWNER TO postgres;

--
-- Name: lab_fun_revoke_refresh_token(uuid); Type: FUNCTION; Schema: public; Owner: postgres
--

CREATE FUNCTION public.lab_fun_revoke_refresh_token(p_token_id uuid) RETURNS boolean
    LANGUAGE plpgsql
    AS $$
DECLARE
  v_count int;
BEGIN
  UPDATE lab_refresh_tokens
     SET revoked = true
   WHERE token_id = p_token_id
     AND revoked = false
  RETURNING 1 INTO v_count;

  RETURN COALESCE(v_count, 0) = 1;
END;
$$;


ALTER FUNCTION public.lab_fun_revoke_refresh_token(p_token_id uuid) OWNER TO postgres;

--
-- Name: lab_fun_touch_updated_at(); Type: FUNCTION; Schema: public; Owner: postgres
--

CREATE FUNCTION public.lab_fun_touch_updated_at() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
  NEW.updated_at := now();
  RETURN NEW;
END; $$;


ALTER FUNCTION public.lab_fun_touch_updated_at() OWNER TO postgres;

--
-- Name: lab_fun_transfer(uuid, uuid, uuid, double precision, text); Type: FUNCTION; Schema: public; Owner: postgres
--

CREATE FUNCTION public.lab_fun_transfer(p_user_id uuid, p_from_account uuid, p_to_account uuid, p_amount double precision, p_description text) RETURNS TABLE(journal_id_credit uuid, journal_id_debit uuid)
    LANGUAGE plpgsql
    AS $$
DECLARE
  v_owner_from    uuid;
  v_owner_to      uuid;
  v_bal_from      numeric;
  v_bal_to        numeric;
  v_new_from      numeric;
  v_new_to        numeric;
  v_j_credit      uuid := gen_random_uuid(); -- jurnal keluar (credit) dari rekening sumber
  v_j_debit       uuid := gen_random_uuid(); -- jurnal masuk (debit) ke rekening tujuan
BEGIN
  -- Validasi nominal
  IF p_amount IS NULL OR p_amount <= 0 THEN
    RAISE EXCEPTION 'AMOUNT_INVALID';
  END IF;

  -- Tidak boleh transfer ke rekening yang sama
  IF p_from_account = p_to_account THEN
    RAISE EXCEPTION 'SAME_ACCOUNT';
  END IF;

  -- Ambil & lock rekening sumber
  SELECT user_id, saldo
    INTO v_owner_from, v_bal_from
  FROM lab_accounts
  WHERE id = p_from_account
  FOR UPDATE;
  IF v_owner_from IS NULL THEN
    RAISE EXCEPTION 'ACCOUNT_FROM_NOT_FOUND';
  END IF;
  IF v_owner_from <> p_user_id THEN
    RAISE EXCEPTION 'ACCOUNT_NOT_OWNED';
  END IF;

  -- Cek saldo cukup
  IF v_bal_from < p_amount THEN
    RAISE EXCEPTION 'INSUFFICIENT_FUNDS';
  END IF;

  -- Ambil & lock rekening tujuan
  SELECT user_id, saldo
    INTO v_owner_to, v_bal_to
  FROM lab_accounts
  WHERE id = p_to_account
  FOR UPDATE;
  IF v_owner_to IS NULL THEN
    RAISE EXCEPTION 'ACCOUNT_TO_NOT_FOUND';
  END IF;

  -- Hitung saldo baru
  v_new_from := v_bal_from - (p_amount::numeric);
  v_new_to   := v_bal_to   + (p_amount::numeric);

  -- Update saldo
  UPDATE lab_accounts SET saldo = v_new_from WHERE id = p_from_account;
  UPDATE lab_accounts SET saldo = v_new_to   WHERE id = p_to_account;

  -- Jurnal CREDIT (keluar) untuk rekening sumber
  INSERT INTO lab_journals(
    id, user_id, account_id, debit, credit, description, balance_after, trx_time
  )
  VALUES(
    v_j_credit, p_user_id, p_from_account,
    0, (p_amount::numeric),
    COALESCE(p_description,'transfer out'),
    v_new_from, now()
  );

  -- Jurnal DEBIT (masuk) untuk rekening tujuan
  INSERT INTO lab_journals(
    id, user_id, account_id, debit, credit, description, balance_after, trx_time
  )
  VALUES(
    v_j_debit, v_owner_to, p_to_account,
    (p_amount::numeric), 0,
    COALESCE(p_description,'transfer in'),
    v_new_to, now()
  );

  -- Kembalikan kedua ID jurnal sesuai yang diharapkan Rust
  journal_id_credit := v_j_credit;
  journal_id_debit  := v_j_debit;
  RETURN NEXT;
END;
$$;


ALTER FUNCTION public.lab_fun_transfer(p_user_id uuid, p_from_account uuid, p_to_account uuid, p_amount double precision, p_description text) OWNER TO postgres;

--
-- Name: lab_fun_update_account_pin(uuid, uuid, text); Type: FUNCTION; Schema: public; Owner: postgres
--

CREATE FUNCTION public.lab_fun_update_account_pin(p_user_id uuid, p_account_id uuid, p_new_pin text) RETURNS boolean
    LANGUAGE plpgsql
    AS $_$
DECLARE
  v_owner uuid;
BEGIN
  IF p_new_pin IS NULL OR length(p_new_pin) <> 6 OR p_new_pin !~ '^[0-9]{6}$' THEN
    RETURN false;
  END IF;

  SELECT user_id INTO v_owner FROM lab_accounts WHERE id = p_account_id;
  IF v_owner IS NULL OR v_owner <> p_user_id THEN
    RETURN false;
  END IF;

  UPDATE lab_accounts
     SET pin = p_new_pin
   WHERE id = p_account_id;

  RETURN FOUND;
END; $_$;


ALTER FUNCTION public.lab_fun_update_account_pin(p_user_id uuid, p_account_id uuid, p_new_pin text) OWNER TO postgres;

--
-- Name: lab_fun_upsert_profile(uuid, text, text, text, date, text, text, text, text); Type: FUNCTION; Schema: public; Owner: postgres
--

CREATE FUNCTION public.lab_fun_upsert_profile(p_user_id uuid, p_ktp_nik text, p_nama_lengkap text, p_tempat_lahir text, p_tanggal_lahir date, p_jenis_kelamin text, p_no_telepon text, p_alamat text, p_ibu_kandung text) RETURNS void
    LANGUAGE plpgsql
    AS $$
BEGIN
  INSERT INTO lab_profiles(user_id, ktp_nik, nama_lengkap, tempat_lahir, tanggal_lahir,
                           jenis_kelamin, no_telepon, alamat, ibu_kandung)
  VALUES (p_user_id, p_ktp_nik, p_nama_lengkap, p_tempat_lahir, p_tanggal_lahir,
          p_jenis_kelamin, p_no_telepon, p_alamat, p_ibu_kandung)
  ON CONFLICT(user_id) DO UPDATE SET
    ktp_nik       = EXCLUDED.ktp_nik,
    nama_lengkap  = EXCLUDED.nama_lengkap,
    tempat_lahir  = EXCLUDED.tempat_lahir,
    tanggal_lahir = EXCLUDED.tanggal_lahir,
    jenis_kelamin = EXCLUDED.jenis_kelamin,
    no_telepon    = EXCLUDED.no_telepon,
    alamat        = EXCLUDED.alamat,
    ibu_kandung   = EXCLUDED.ibu_kandung,
    updated_at    = now();
END; $$;


ALTER FUNCTION public.lab_fun_upsert_profile(p_user_id uuid, p_ktp_nik text, p_nama_lengkap text, p_tempat_lahir text, p_tanggal_lahir date, p_jenis_kelamin text, p_no_telepon text, p_alamat text, p_ibu_kandung text) OWNER TO postgres;

--
-- Name: lab_fun_verify_account_pin(uuid, uuid, text); Type: FUNCTION; Schema: public; Owner: postgres
--

CREATE FUNCTION public.lab_fun_verify_account_pin(p_user_id uuid, p_account_id uuid, p_pin text) RETURNS boolean
    LANGUAGE plpgsql
    AS $$
DECLARE
  v_owner uuid;
  v_hash  text;
BEGIN
  SELECT user_id, pin
    INTO v_owner, v_hash
    FROM lab_accounts
   WHERE (id = p_account_id or account_no = p_account_id::varchar);

  IF v_owner IS NULL OR v_owner <> p_user_id THEN
    RETURN FALSE;
  END IF;

  RETURN crypt(p_pin, v_hash) = v_hash;
END;
$$;


ALTER FUNCTION public.lab_fun_verify_account_pin(p_user_id uuid, p_account_id uuid, p_pin text) OWNER TO postgres;

--
-- Name: lab_fun_withdraw(uuid, uuid, double precision, text); Type: FUNCTION; Schema: public; Owner: postgres
--

CREATE FUNCTION public.lab_fun_withdraw(p_user_id uuid, p_account_id uuid, p_amount double precision, p_description text) RETURNS TABLE(journal_id uuid, account_id uuid, balance_after double precision, trx_time timestamp with time zone, description text)
    LANGUAGE plpgsql
    AS $$
DECLARE
  v_owner uuid;
  v_balance double precision;
  v_journal uuid := gen_random_uuid();
  v_desc text := COALESCE(p_description, 'Tarik tunai');
BEGIN
  IF p_amount IS NULL OR p_amount <= 0 THEN
    RAISE EXCEPTION 'AMOUNT_INVALID';
  END IF;

  SELECT user_id INTO v_owner FROM lab_accounts WHERE id = p_account_id;
  IF v_owner IS NULL OR v_owner <> p_user_id THEN
    RAISE EXCEPTION 'ACCOUNT_NOT_OWNED';
  END IF;

  SELECT saldo INTO v_balance FROM lab_accounts WHERE id = p_account_id FOR UPDATE;
  IF v_balance < p_amount THEN
    RAISE EXCEPTION 'INSUFFICIENT_FUNDS';
  END IF;

  -- kurangi saldo
  UPDATE lab_accounts
     SET saldo = saldo - p_amount
   WHERE id = p_account_id
   RETURNING saldo INTO v_balance;

  -- jurnal credit
  INSERT INTO lab_journals(id, user_id, account_id, debit, credit, description, balance_after, trx_time)
  VALUES (v_journal, p_user_id, p_account_id, 0, p_amount, v_desc, v_balance, now());

  INSERT INTO lab_audit_logs(id, user_id, action, ip_addr, user_agent, created_at)
  VALUES (gen_random_uuid(), p_user_id, 'WITHDRAW', NULL, NULL, now());

  RETURN QUERY
  SELECT v_journal, p_account_id, v_balance, now(), v_desc;
END;
$$;


ALTER FUNCTION public.lab_fun_withdraw(p_user_id uuid, p_account_id uuid, p_amount double precision, p_description text) OWNER TO postgres;

--
-- Name: lab_touch_updated_at(); Type: FUNCTION; Schema: public; Owner: postgres
--

CREATE FUNCTION public.lab_touch_updated_at() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
  NEW.updated_at = now();
  RETURN NEW;
END; $$;


ALTER FUNCTION public.lab_touch_updated_at() OWNER TO postgres;

SET default_tablespace = '';

SET default_table_access_method = heap;

-- =========================================================
-- Transfer by account_no (tanpa mengubah fungsi lama)
-- =========================================================
-- DROP FUNCTION public.lab_fun_transfer_by_no(uuid, text, text, double precision, text);

CREATE OR REPLACE FUNCTION public.lab_fun_transfer_by_no(
  p_user_id        uuid,
  p_from_account_no text,
  p_to_account_no   text,
  p_amount          double precision,
  p_description     text
)
RETURNS TABLE(journal_id_credit uuid, journal_id_debit uuid)
LANGUAGE plpgsql
AS $$
DECLARE
  v_from_id     uuid;
  v_to_id       uuid;

  v_owner_from  uuid;
  v_owner_to    uuid;

  v_bal_from    numeric;
  v_bal_to      numeric;

  v_new_from    numeric;
  v_new_to      numeric;

  v_j_credit    uuid := gen_random_uuid();
  v_j_debit     uuid := gen_random_uuid();
BEGIN
  IF p_amount IS NULL OR p_amount <= 0 THEN
    RAISE EXCEPTION 'AMOUNT_INVALID';
  END IF;

  -- Resolve account_no -> id (+ lock)
  SELECT id, user_id, saldo
    INTO v_from_id, v_owner_from, v_bal_from
  FROM lab_accounts
  WHERE account_no = p_from_account_no
  FOR UPDATE;
  IF v_from_id IS NULL THEN
    RAISE EXCEPTION 'ACCOUNT_FROM_NOT_FOUND';
  END IF;
  IF v_owner_from <> p_user_id THEN
    RAISE EXCEPTION 'ACCOUNT_NOT_OWNED';
  END IF;

  SELECT id, user_id, saldo
    INTO v_to_id, v_owner_to, v_bal_to
  FROM lab_accounts
  WHERE account_no = p_to_account_no
  FOR UPDATE;
  IF v_to_id IS NULL THEN
    RAISE EXCEPTION 'ACCOUNT_TO_NOT_FOUND';
  END IF;

  IF v_from_id = v_to_id THEN
    RAISE EXCEPTION 'SAME_ACCOUNT';
  END IF;

  IF v_bal_from < p_amount THEN
    RAISE EXCEPTION 'INSUFFICIENT_FUNDS';
  END IF;

  -- Hitung saldo baru
  v_new_from := v_bal_from - (p_amount::numeric);
  v_new_to   := v_bal_to   + (p_amount::numeric);

  -- Update saldo
  UPDATE lab_accounts SET saldo = v_new_from WHERE id = v_from_id;
  UPDATE lab_accounts SET saldo = v_new_to   WHERE id = v_to_id;

  -- Jurnal CREDIT (keluar) sumber
  INSERT INTO lab_journals (id, user_id, account_id, debit, credit, description, balance_after, trx_time)
  VALUES (v_j_credit, p_user_id, v_from_id, 0, (p_amount::numeric), COALESCE(p_description,'transfer out'), v_new_from, now());

  -- Jurnal DEBIT (masuk) tujuan
  INSERT INTO lab_journals (id, user_id, account_id, debit, credit, description, balance_after, trx_time)
  VALUES (v_j_debit, v_owner_to, v_to_id, (p_amount::numeric), 0, COALESCE(p_description,'transfer in'), v_new_to, now());

  journal_id_credit := v_j_credit;
  journal_id_debit  := v_j_debit;
  RETURN NEXT;
END;
$$;

--
-- Name: lab_accounts; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.lab_accounts (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    user_id uuid NOT NULL,
    account_no character varying(14) NOT NULL,
    pin character(1000) NOT NULL,
    saldo numeric(20,2) DEFAULT 0 NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);


ALTER TABLE public.lab_accounts OWNER TO postgres;

--
-- Name: lab_audit_logs; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.lab_audit_logs (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    user_id uuid,
    action text NOT NULL,
    target text,
    meta jsonb,
    ip_addr text,
    user_agent text
);


ALTER TABLE public.lab_audit_logs OWNER TO postgres;

--
-- Name: lab_journals; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.lab_journals (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    user_id uuid NOT NULL,
    account_id uuid NOT NULL,
    trx_time timestamp with time zone DEFAULT now() NOT NULL,
    debit numeric(20,2) DEFAULT 0 NOT NULL,
    credit numeric(20,2) DEFAULT 0 NOT NULL,
    description text,
    balance_after numeric(20,2) NOT NULL
);


ALTER TABLE public.lab_journals OWNER TO postgres;

--
-- Name: lab_profiles; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.lab_profiles (
    user_id uuid NOT NULL,
    ktp_nik character varying(32),
    nama_lengkap text,
    tempat_lahir text,
    tanggal_lahir date,
    jenis_kelamin text,
    no_telepon text,
    alamat text,
    ibu_kandung text,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT lab_profiles_jenis_kelamin_check CHECK ((jenis_kelamin = ANY (ARRAY['L'::text, 'P'::text])))
);


ALTER TABLE public.lab_profiles OWNER TO postgres;

--
-- Name: lab_refresh_tokens; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.lab_refresh_tokens (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    user_id uuid NOT NULL,
    token_hash text,
    user_agent text,
    ip_addr text,
    expires_at timestamp with time zone NOT NULL,
    revoked_at timestamp with time zone,
    token_sha256 bytea,
    rotated_from uuid,
    token_id uuid NOT NULL,
    revoked boolean DEFAULT false NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);


ALTER TABLE public.lab_refresh_tokens OWNER TO postgres;

--
-- Name: lab_revoked_access_tokens; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.lab_revoked_access_tokens (
    jti uuid NOT NULL,
    expires_at timestamp with time zone NOT NULL
);


ALTER TABLE public.lab_revoked_access_tokens OWNER TO postgres;

--
-- Name: lab_users; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.lab_users (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    email public.citext NOT NULL,
    password_hash text NOT NULL,
    role text NOT NULL,
    is_active boolean DEFAULT true NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT lab_users_role_check CHECK ((role = ANY (ARRAY['admin'::text, 'user'::text])))
);


ALTER TABLE public.lab_users OWNER TO postgres;

--
-- Name: lab_accounts lab_accounts_account_no_key; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.lab_accounts
    ADD CONSTRAINT lab_accounts_account_no_key UNIQUE (account_no);


--
-- Name: lab_accounts lab_accounts_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.lab_accounts
    ADD CONSTRAINT lab_accounts_pkey PRIMARY KEY (id);


--
-- Name: lab_audit_logs lab_audit_logs_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.lab_audit_logs
    ADD CONSTRAINT lab_audit_logs_pkey PRIMARY KEY (id);


--
-- Name: lab_journals lab_journals_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.lab_journals
    ADD CONSTRAINT lab_journals_pkey PRIMARY KEY (id);


--
-- Name: lab_profiles lab_profiles_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.lab_profiles
    ADD CONSTRAINT lab_profiles_pkey PRIMARY KEY (user_id);


--
-- Name: lab_refresh_tokens lab_refresh_tokens_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.lab_refresh_tokens
    ADD CONSTRAINT lab_refresh_tokens_pkey PRIMARY KEY (id);


--
-- Name: lab_revoked_access_tokens lab_revoked_access_tokens_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.lab_revoked_access_tokens
    ADD CONSTRAINT lab_revoked_access_tokens_pkey PRIMARY KEY (jti);


--
-- Name: lab_users lab_users_email_key; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.lab_users
    ADD CONSTRAINT lab_users_email_key UNIQUE (email);


--
-- Name: lab_users lab_users_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.lab_users
    ADD CONSTRAINT lab_users_pkey PRIMARY KEY (id);


--
-- Name: idx_audit_action; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_audit_action ON public.lab_audit_logs USING btree (action);


--
-- Name: idx_audit_ts; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_audit_ts ON public.lab_audit_logs USING btree (created_at DESC);


--
-- Name: idx_audit_user_ts; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_audit_user_ts ON public.lab_audit_logs USING btree (user_id, created_at DESC);


--
-- Name: idx_lab_accounts_user; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_lab_accounts_user ON public.lab_accounts USING btree (user_id);


--
-- Name: idx_lab_journals_account_time; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_lab_journals_account_time ON public.lab_journals USING btree (account_id, trx_time DESC);


--
-- Name: idx_lab_journals_user_acc; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_lab_journals_user_acc ON public.lab_journals USING btree (user_id, account_id, trx_time DESC);


--
-- Name: idx_lab_refresh_tokens_sha256; Type: INDEX; Schema: public; Owner: postgres
--

CREATE UNIQUE INDEX idx_lab_refresh_tokens_sha256 ON public.lab_refresh_tokens USING btree (token_sha256) WHERE (revoked = false);


--
-- Name: idx_lab_refresh_tokens_token_id; Type: INDEX; Schema: public; Owner: postgres
--

CREATE UNIQUE INDEX idx_lab_refresh_tokens_token_id ON public.lab_refresh_tokens USING btree (token_id);


--
-- Name: idx_revoked_access_tokens_exp; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_revoked_access_tokens_exp ON public.lab_revoked_access_tokens USING btree (expires_at);


--
-- Name: lab_accounts lab_accounts_touch; Type: TRIGGER; Schema: public; Owner: postgres
--

CREATE TRIGGER lab_accounts_touch BEFORE UPDATE ON public.lab_accounts FOR EACH ROW EXECUTE FUNCTION public.lab_touch_updated_at();


--
-- Name: lab_profiles lab_profiles_touch; Type: TRIGGER; Schema: public; Owner: postgres
--

CREATE TRIGGER lab_profiles_touch BEFORE UPDATE ON public.lab_profiles FOR EACH ROW EXECUTE FUNCTION public.lab_touch_updated_at();


--
-- Name: lab_users lab_users_touch; Type: TRIGGER; Schema: public; Owner: postgres
--

CREATE TRIGGER lab_users_touch BEFORE UPDATE ON public.lab_users FOR EACH ROW EXECUTE FUNCTION public.lab_touch_updated_at();


--
-- Name: lab_accounts trg_lab_accounts_touch; Type: TRIGGER; Schema: public; Owner: postgres
--

CREATE TRIGGER trg_lab_accounts_touch BEFORE UPDATE ON public.lab_accounts FOR EACH ROW EXECUTE FUNCTION public.lab_fun_touch_updated_at();


--
-- Name: lab_journals trg_lab_journal_guard; Type: TRIGGER; Schema: public; Owner: postgres
--

CREATE TRIGGER trg_lab_journal_guard BEFORE INSERT ON public.lab_journals FOR EACH ROW EXECUTE FUNCTION public.lab_fun_journal_guard();


--
-- Name: lab_accounts lab_accounts_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.lab_accounts
    ADD CONSTRAINT lab_accounts_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.lab_users(id) ON DELETE CASCADE;


--
-- Name: lab_journals lab_journals_account_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.lab_journals
    ADD CONSTRAINT lab_journals_account_id_fkey FOREIGN KEY (account_id) REFERENCES public.lab_accounts(id) ON DELETE CASCADE;


--
-- Name: lab_journals lab_journals_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.lab_journals
    ADD CONSTRAINT lab_journals_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.lab_users(id) ON DELETE CASCADE;


--
-- Name: lab_profiles lab_profiles_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.lab_profiles
    ADD CONSTRAINT lab_profiles_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.lab_users(id) ON DELETE CASCADE;


--
-- Name: lab_refresh_tokens lab_refresh_tokens_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.lab_refresh_tokens
    ADD CONSTRAINT lab_refresh_tokens_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.lab_users(id) ON DELETE CASCADE;


--
-- PostgreSQL database dump complete
--

\unrestrict smLsFw4SxJ9gqvYN4OPg3fkdfmBBmf9Wbjz9ddz1RSF1mvDrMcQ5xhqMvizhMiG

