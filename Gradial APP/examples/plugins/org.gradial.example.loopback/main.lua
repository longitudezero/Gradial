local plugin = {}

function plugin.begin(context, parameters, initial)
    return { value = initial }
end

function plugin.update(session, sample)
    session.value = sample.value
end

function plugin.commit(session, sample)
    session.value = sample.value
    local body = string.format('{"value":%.6f}', session.value)
    local accepted, reason = gradial.network.loopback({
        method = "POST",
        origin = "http://127.0.0.1:32123",
        path = "/gradial/example",
        body = body,
        content_type = "application/json",
    })
    session.request_status = accepted and "accepted" or reason
end

function plugin.cancel(session, reason)
    session.request_status = "cancelled"
end

return plugin
