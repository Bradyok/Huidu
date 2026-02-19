#!/bin/sh

# set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR" || exit 1

SERVICE_NAME="cn.huidu.device.service"
SCRIPT_NAME="$(basename "$0")"
OMS_ENABLE_FILE="/boot/omsEnable"

print_green()  { echo -e "\033[32m$1\033[0m"; }
print_red()    { echo -e "\033[31m$1\033[0m"; }
print_yellow() { echo -e "\033[33m$1\033[0m"; }
print_blue()   { echo -e "\033[34m$1\033[0m"; }

init_environment() {
    [ -f "./$SERVICE_NAME" ] && chmod +x "./$SERVICE_NAME" 2>/dev/null || true
}

check_oms_enable() {
    [ -f "$OMS_ENABLE_FILE" ]
}

get_service_pids() {
    pids=$(pgrep -f "^$(pwd)/$SERVICE_NAME\$" 2>/dev/null)
    [ -z "$pids" ] && pids=$(ps aux | grep -v grep | grep -F "$SERVICE_NAME" | awk '{print $2}')
    echo "$pids" | tr ' ' '\n' | sort -u | tr '\n' ' ' | sed 's/ $//'
}

stop_service() {
    print_yellow "Stopping $SERVICE_NAME..."

    pids=$(get_service_pids)
    if [ -z "$pids" ]; then
        print_blue "Service not running"
        return 0
    fi

    for pid in $pids; do
        [ "$pid" = "$$" ] && continue
        kill -15 "$pid" 2>/dev/null || true
        echo "Sent TERM to PID $pid"
    done

    sleep 2

    pids=$(get_service_pids)
    for pid in $pids; do
        [ "$pid" = "$$" ] && continue
        print_yellow "Forcing stop PID $pid..."
        kill -9 "$pid" 2>/dev/null || true
    done

    print_green "Service stopped"
}

start_permanent() {
    print_blue "Starting $SERVICE_NAME (permanent mode)..."

    if ! check_oms_enable; then
        print_red "ERROR: /boot/omsEnable file not found!"
        print_yellow "Hint: Use temporary mode or create /boot/omsEnable"
        return 1
    fi

	stop_service
    sleep 1

    if [ ! -f "./$SERVICE_NAME" ]; then
        print_red "ERROR: Service executable not found!"
        return 1
    fi

    nohup "./$SERVICE_NAME" > /dev/null 2>&1 &
    service_pid=$!

    sleep 2
    if [ -n "$(get_service_pids)" ]; then
        print_green "Service started successfully (PID: $service_pid)"
        return 0
    else
        print_red "ERROR: Service failed to start!"
        return 1
    fi
}

start_temporary() {
    timeout=${1:-300}
    print_blue "Starting $SERVICE_NAME (temporary mode, ${timeout}s)..."

	stop_service
    sleep 1

    if [ ! -f "./$SERVICE_NAME" ]; then
        print_red "ERROR: Service executable not found!"
        return 1
    fi

    nohup "./$SERVICE_NAME" > /dev/null 2>&1 &
    service_pid=$!

    sleep 2
    if [ -z "$(get_service_pids)" ]; then
        print_red "ERROR: Service failed to start!"
        return 1
    fi

    print_green "Service is running (PID: $service_pid)"

    # BusyBox 安全 timeout
    (
        sleep "$timeout"
        if kill -0 "$service_pid" 2>/dev/null; then
            kill -15 "$service_pid" 2>/dev/null || true
            sleep 2
            kill -9 "$service_pid" 2>/dev/null || true
        fi
    ) >/dev/null 2>&1 &

    print_yellow "Service will stop automatically after ${timeout} seconds"
    print_blue "Use './$SCRIPT_NAME stop' to stop immediately"
}

show_usage() {
    echo ""
    print_blue "=== $SCRIPT_NAME - Service Manager ==="
    echo ""
    echo "Usage: $0 {permanent|temporary [seconds]|stop}"
    echo ""
    echo "Commands:"
    echo "  permanent              Start service permanently"
    echo "  temporary [sec]        Start temporarily (default 300s)"
    echo "  stop                   Stop service"
    echo ""
}

main() {
    init_environment
    
    case "${1:-}" in
        permanent)
            start_permanent
            ;;
        temporary)
            start_temporary "$2"
            ;;
        stop)
            stop_service
            ;;
        help|--help|-h)
            show_usage
            ;;
        *)
            print_red "Unknown command: $1"
            show_usage
            return 1
            ;;
    esac
	
    return $?
}

trap 'print_red "Script interrupted!"; exit 1' INT

main "$@"
exit $?
