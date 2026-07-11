from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...types import Response, UNSET
from ... import errors

from ...models.auto_resolution_entry_response import AutoResolutionEntryResponse
from ...models.submit_auto_resolution_request import SubmitAutoResolutionRequest
from typing import cast



def _get_kwargs(
    *,
    body: SubmitAutoResolutionRequest,

) -> dict[str, Any]:
    headers: dict[str, Any] = {}


    

    

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v1/admin/auto-resolutions",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs



def _parse_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Any | AutoResolutionEntryResponse | None:
    if response.status_code == 200:
        response_200 = AutoResolutionEntryResponse.from_dict(response.json())



        return response_200

    if response.status_code == 400:
        response_400 = cast(Any, None)
        return response_400

    if response.status_code == 403:
        response_403 = cast(Any, None)
        return response_403

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Response[Any | AutoResolutionEntryResponse]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: SubmitAutoResolutionRequest,

) -> Response[Any | AutoResolutionEntryResponse]:
    """ POST /v1/admin/auto-resolutions — record (or refresh) an auto-resolution
    proposal. Never settles a market.

    Args:
        body (SubmitAutoResolutionRequest): Body of `POST /v1/admin/auto-resolutions` (SYB-48).
            The auto-resolution
            resolver (sybil-polymarket) submits one of these per market it has
            evaluated with an LLM. This route NEVER settles a market: it only records a
            reviewable proposal. Finalization always flows back through the existing
            signed `POST /v1/markets/{id}/resolve` money path.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | AutoResolutionEntryResponse]
     """


    kwargs = _get_kwargs(
        body=body,

    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)

def sync(
    *,
    client: AuthenticatedClient | Client,
    body: SubmitAutoResolutionRequest,

) -> Any | AutoResolutionEntryResponse | None:
    """ POST /v1/admin/auto-resolutions — record (or refresh) an auto-resolution
    proposal. Never settles a market.

    Args:
        body (SubmitAutoResolutionRequest): Body of `POST /v1/admin/auto-resolutions` (SYB-48).
            The auto-resolution
            resolver (sybil-polymarket) submits one of these per market it has
            evaluated with an LLM. This route NEVER settles a market: it only records a
            reviewable proposal. Finalization always flows back through the existing
            signed `POST /v1/markets/{id}/resolve` money path.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | AutoResolutionEntryResponse
     """


    return sync_detailed(
        client=client,
body=body,

    ).parsed

async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: SubmitAutoResolutionRequest,

) -> Response[Any | AutoResolutionEntryResponse]:
    """ POST /v1/admin/auto-resolutions — record (or refresh) an auto-resolution
    proposal. Never settles a market.

    Args:
        body (SubmitAutoResolutionRequest): Body of `POST /v1/admin/auto-resolutions` (SYB-48).
            The auto-resolution
            resolver (sybil-polymarket) submits one of these per market it has
            evaluated with an LLM. This route NEVER settles a market: it only records a
            reviewable proposal. Finalization always flows back through the existing
            signed `POST /v1/markets/{id}/resolve` money path.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | AutoResolutionEntryResponse]
     """


    kwargs = _get_kwargs(
        body=body,

    )

    response = await client.get_async_httpx_client().request(
        **kwargs
    )

    return _build_response(client=client, response=response)

async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    body: SubmitAutoResolutionRequest,

) -> Any | AutoResolutionEntryResponse | None:
    """ POST /v1/admin/auto-resolutions — record (or refresh) an auto-resolution
    proposal. Never settles a market.

    Args:
        body (SubmitAutoResolutionRequest): Body of `POST /v1/admin/auto-resolutions` (SYB-48).
            The auto-resolution
            resolver (sybil-polymarket) submits one of these per market it has
            evaluated with an LLM. This route NEVER settles a market: it only records a
            reviewable proposal. Finalization always flows back through the existing
            signed `POST /v1/markets/{id}/resolve` money path.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | AutoResolutionEntryResponse
     """


    return (await asyncio_detailed(
        client=client,
body=body,

    )).parsed
