local function band_for(value, band_count)
    return math.min(band_count - 1, math.floor(value * band_count))
end

local function step_toward_sample(session, sample)
    local next_band = band_for(sample.value, session.band_count)
    if next_band == session.current_band then
        return
    end

    local shortcut = session.decrease_shortcut
    local completed_band = session.current_band - 1
    if next_band > session.current_band then
        shortcut = session.increase_shortcut
        completed_band = session.current_band + 1
    end

    local accepted, reason = gradial.input.shortcut(shortcut)
    if accepted then
        session.current_band = completed_band
        session.last_rejection = nil
    else
        session.last_rejection = reason
    end
end

return {
    begin = function(context, parameters, initial)
        return {
            action_id = context.action_id,
            band_count = parameters.bands,
            increase_shortcut = parameters.increase_shortcut,
            decrease_shortcut = parameters.decrease_shortcut,
            current_band = band_for(initial.value, parameters.bands),
        }
    end,

    update = function(session, sample)
        step_toward_sample(session, sample)
    end,

    commit = function(session, final_sample)
        step_toward_sample(session, final_sample)
    end,

    cancel = function(session, reason)
        session.cancel_reason = reason
    end,
}
