#!/usr/bin/env bash
# Four throwaway Samba servers on 127.0.0.1 for testing the SMB client.
#
#   scripts/smb-test-servers.sh start <dir>
#   scripts/smb-test-servers.sh stop <dir>
#
#   port 4451  SMB 1 only (NT1)
#   port 4452  SMB 2.0.2 to 2.1
#   port 4453  SMB 3 only, encryption required
#   port 4454  SMB 1 with signing required
#
# Each has a read-only share "test" (<dir>/share, the same for all four) and
# a writable share "rw" (<dir>/rwN). The account is $USER, password sqdtest.
# Keep <dir> short (60 characters at most). Runs as you, not root: no system config, nothing outside <dir>. Needs
# smbd and pdbedit (the samba package). stop kills only the servers whose
# PIDs are in <dir>/srvN/pid.txt.
set -euo pipefail
cmd=$1
dir=$(mkdir -p "$2" && cd "$2" && pwd)
# smbd's sockets live under <dir>, and a socket path can't be much longer
[ ${#dir} -le 60 ] || { echo "$dir is too long for smbd's sockets, use a shorter path" >&2; exit 1; }

stop() {
    for n in 1 2 3 4; do
        f=$dir/srv$n/pid.txt
        [ -f "$f" ] || continue
        pid=$(cat "$f")
        # the main smbd's own children (notifyd, cleanupd) go with it
        kill $(pgrep -P "$pid") "$pid" 2>/dev/null || true
        rm -f "$f"
    done
}

case $cmd in
stop) stop; exit ;;
start) ;;
*) echo "usage: $0 start|stop <dir>" >&2; exit 2 ;;
esac

stop
# the same files on every server; a-copy.jpg duplicates a.jpg
mkdir -p "$dir/share/docs" "$dir/share/photos/2024" "$dir/share/with space"
head -c 1200000 /dev/urandom >"$dir/share/docs/big.bin"
head -c 300000 /dev/urandom >"$dir/share/photos/2024/a.jpg"
cp "$dir/share/photos/2024/a.jpg" "$dir/share/docs/a-copy.jpg"
head -c 50000 /dev/urandom >"$dir/share/photos/small.png"
printf 'hello\n' >"$dir/share/with space/note.txt"

for n in 1 2 3 4; do
    s=$dir/srv$n
    mkdir -p "$s"/{private,lock,state/ncalrpc,cache,pid,log} "$dir/rw$n"
    case $n in
    1) proto=$'server min protocol = NT1\n   server max protocol = NT1\n   ntlm auth = ntlmv1-permitted' ;;
    2) proto=$'server min protocol = SMB2_02\n   server max protocol = SMB2_10' ;;
    3) proto=$'server min protocol = SMB3_00\n   server max protocol = SMB3_11\n   smb encrypt = required' ;;
    4) proto=$'server min protocol = NT1\n   server max protocol = NT1\n   ntlm auth = ntlmv1-permitted\n   server signing = mandatory' ;;
    esac
    cat >"$s/smb.conf" <<EOF
[global]
   smb ports = 445$n
   netbios name = SQDTEST$n
   workgroup = WORKGROUP
   $proto
   private dir = $s/private
   lock directory = $s/lock
   state directory = $s/state
   cache directory = $s/cache
   pid directory = $s/pid
   ncalrpc dir = $s/state/ncalrpc
   log file = $s/log/log.%m
   log level = 1
   passdb backend = tdbsam:$s/private/passdb.tdb
   interfaces = 127.0.0.1
   bind interfaces only = yes
   disable spoolss = yes
   load printers = no
   printing = bsd
   printcap name = /dev/null
   server role = standalone server
   map to guest = never

[test]
   path = $dir/share
   read only = yes
   valid users = $USER

[rw]
   path = $dir/rw$n
   read only = no
   valid users = $USER
EOF
    printf 'sqdtest\nsqdtest\n' | pdbedit -s "$s/smb.conf" -a -u "$USER" -t >/dev/null
    # in its own session, so it outlives this shell
    setsid /usr/sbin/smbd --foreground --no-process-group --debug-stdout -l "$s/log" -s "$s/smb.conf" >"$s/log/stdout" 2>&1 &
    echo $! >"$s/pid.txt"
done
echo "SMB test servers on 127.0.0.1:4451-4454. Stop them with: $0 stop $dir"
